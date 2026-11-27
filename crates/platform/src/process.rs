//! Engine process lifetime: Job Object on Windows, process group on Linux.
//!
//! The launcher owns the whole process tree. QMP shutdown runs first, then a
//! watchdog forces termination of the group or job so no
//! `reims-vgpu-compile` process is left behind.

// SAFETY scope: Win32 Job Object calls below are the only unsafe operations in
// this module; each call site documents its handle invariant.
#![allow(unsafe_code, reason = "Win32 Job Object FFI boundary")]

#[cfg(windows)]
use std::ffi::c_void;
use std::path::Path;
use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;

use reims_core::qmp::{QmpClient, SHUTDOWN_TIMEOUT};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStderr, Command};

use crate::adapter::CommandSpec;

/// Watchdog used after a graceful QMP shutdown request.
pub const SHUTDOWN_WATCHDOG: Duration = SHUTDOWN_TIMEOUT;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;

/// Failures while starting or controlling the engine process.
#[derive(Debug, Error)]
pub enum ProcessError {
    /// The engine binary could not be started.
    #[error("cannot spawn {program}: {source}")]
    Spawn {
        /// Program from the [`CommandSpec`].
        program: String,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// Waiting for or signalling the process failed.
    #[error("process control failed: {0}")]
    Control(#[from] std::io::Error),
    /// A Win32 Job Object operation failed.
    #[error("job object error: {0}")]
    JobObject(String),
}

/// Builds the command that starts the engine.
///
/// The working directory is the VM folder inside the portable tree; all
/// engine paths in the command are relative to it.
///
/// # Errors
///
/// Returns [`ProcessError::Spawn`] only when the program cannot be resolved.
pub fn prepare_command(spec: &CommandSpec, portable_root: &Path) -> Result<Command, ProcessError> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .envs(
            spec.env
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .current_dir(portable_root.join(&spec.working_dir))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command
            .as_std_mut()
            .creation_flags(CREATE_NO_WINDOW | ABOVE_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // The child becomes its own process group leader, so killpg reaches
        // QEMU and every reims-vgpu-compile child it spawns.
        command.as_std_mut().process_group(0);
    }
    Ok(command)
}

#[cfg(windows)]
mod win32 {
    use super::*;

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JobObjectBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JobObjectExtendedLimitInformation {
        basic: JobObjectBasicLimitInformation,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(
            job: *mut c_void,
            class: u32,
            information: *const c_void,
            length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    /// Owns a Job Object configured with `KILL_ON_JOB_CLOSE`.
    pub(super) struct JobHandle(*mut c_void);

    impl std::fmt::Debug for JobHandle {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("JobHandle")
        }
    }

    impl JobHandle {
        pub(super) fn create(process: *mut c_void) -> Result<Self, ProcessError> {
            // SAFETY: CreateJobObjectW accepts null security attributes and a
            // null name to create an unnamed object.
            let job = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
            if job.is_null() {
                return Err(ProcessError::JobObject(String::from(
                    "CreateJobObjectW failed",
                )));
            }
            let job = Self(job);
            // SAFETY: all fields are plain integers and the struct is POD.
            let mut information: JobObjectExtendedLimitInformation = unsafe { std::mem::zeroed() };
            information.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `job` is a live handle and `information` points to a
            // correctly sized and initialized structure.
            let ok = unsafe {
                SetInformationJobObject(
                    job.0,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    std::ptr::from_ref(&information).cast::<c_void>(),
                    size_of::<JobObjectExtendedLimitInformation>() as u32,
                )
            };
            if ok == 0 {
                return Err(ProcessError::JobObject(String::from(
                    "SetInformationJobObject failed",
                )));
            }
            // SAFETY: `process` is a live process handle from the spawned child
            // and the child has not yet been assigned to another job.
            let ok = unsafe { AssignProcessToJobObject(job.0, process) };
            if ok == 0 {
                return Err(ProcessError::JobObject(String::from(
                    "AssignProcessToJobObject failed",
                )));
            }
            Ok(job)
        }

        pub(super) fn terminate(&self) -> Result<(), ProcessError> {
            // SAFETY: `self.0` is a live job handle owned by this struct.
            let ok = unsafe { TerminateJobObject(self.0, 1) };
            if ok == 0 {
                return Err(ProcessError::JobObject(String::from(
                    "TerminateJobObject failed",
                )));
            }
            Ok(())
        }
    }

    impl Drop for JobHandle {
        fn drop(&mut self) {
            // Closing the last handle triggers KILL_ON_JOB_CLOSE; errors cannot
            // be reported from Drop.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

/// A running engine process and its lifetime guard.
#[derive(Debug)]
pub struct VmProcess {
    child: Child,
    #[cfg(unix)]
    pgid: u32,
    #[cfg(windows)]
    job: win32::JobHandle,
    stderr: Option<ChildStderr>,
}

impl VmProcess {
    /// Starts the engine described by `spec` inside `portable_root`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError`] if the process or its lifetime guard cannot
    /// be created.
    pub fn spawn(spec: &CommandSpec, portable_root: &Path) -> Result<Self, ProcessError> {
        let mut command = prepare_command(spec, portable_root)?;
        let program = spec.program.clone();
        let mut child = command
            .spawn()
            .map_err(|source| ProcessError::Spawn { program, source })?;
        let stderr = child.stderr.take();

        #[cfg(unix)]
        let pgid = child
            .id()
            .ok_or_else(|| ProcessError::Control(std::io::Error::other("child has no pid")))?;

        #[cfg(windows)]
        let job = {
            let handle = child.raw_handle().ok_or_else(|| {
                ProcessError::Control(std::io::Error::other("child has no process handle"))
            })?;
            match win32::JobHandle::create(handle) {
                Ok(job) => job,
                Err(error) => {
                    // Do not leave an unmanaged QEMU process behind when the
                    // lifetime guard fails to attach.
                    let _ = child.start_kill();
                    return Err(error);
                }
            }
        };

        Ok(Self {
            child,
            #[cfg(unix)]
            pgid,
            #[cfg(windows)]
            job,
            stderr,
        })
    }

    /// Returns the QEMU PID.
    pub fn id(&self) -> u32 {
        self.child.id().unwrap_or_default()
    }

    /// Returns the unbuffered engine stderr stream, once.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.stderr.take()
    }

    /// Returns the exit status when the process already finished.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Control`] when the OS cannot report a status.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, ProcessError> {
        Ok(self.child.try_wait()?)
    }

    /// Waits up to `timeout` and reports the exit status, or `None` on timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Control`] on a wait failure.
    pub async fn wait_within(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<ExitStatus>, ProcessError> {
        match tokio::time::timeout(timeout, self.child.wait()).await {
            Ok(result) => Ok(Some(result?)),
            Err(_) => Ok(None),
        }
    }

    /// Forces termination of the whole process tree.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError`] when the OS refuses the signal.
    pub fn kill(&mut self) -> Result<(), ProcessError> {
        #[cfg(unix)]
        {
            // SAFETY: `pgid` is a live process group created by this instance.
            let result = unsafe { libc::killpg(self.pgid as i32, libc::SIGKILL) };
            if result != 0 {
                return Err(ProcessError::Control(std::io::Error::last_os_error()));
            }
        }
        #[cfg(windows)]
        self.job.terminate()?;
        let _ = self.child.start_kill();
        Ok(())
    }

    /// Requests QMP shutdown, waits for the watchdog, then kills the tree.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError`] when the final wait fails. QMP failures are
    /// treated as a reason to escalate to `quit` and then to a forced kill.
    pub async fn graceful_shutdown(
        &mut self,
        qmp: &mut QmpClient<TcpStream>,
    ) -> Result<(), ProcessError> {
        if qmp.shutdown().await.is_err() {
            // The socket may close before the reply arrives; try an immediate
            // quit before escalating to a forced kill.
            let _ = qmp.quit().await;
        }
        if self.wait_within(SHUTDOWN_WATCHDOG).await?.is_some() {
            return Ok(());
        }
        self.kill()?;
        self.child.wait().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn spec() -> CommandSpec {
        let mut env = BTreeMap::new();
        env.insert(String::from("REIMS_VGPU_LOG"), String::from("warn"));
        CommandSpec {
            program: String::from("../../engines/test/bin/qemu-system-x86_64"),
            args: vec![String::from("-display"), String::from("none")],
            env,
            working_dir: String::from("vms/macos-tahoe"),
        }
    }

    #[test]
    fn prepares_the_command_with_relative_engine_paths() {
        let root = Path::new("ReimsStudio");
        let command = prepare_command(&spec(), root).expect("command");
        let std_command = command.as_std();
        assert_eq!(
            std_command.get_program(),
            "../../engines/test/bin/qemu-system-x86_64"
        );
        assert_eq!(
            std_command.get_current_dir(),
            Some(root.join("vms/macos-tahoe").as_path())
        );
        assert_eq!(std_command.get_args().count(), 2);
        assert!(
            std_command
                .get_envs()
                .any(|(key, _)| key == "REIMS_VGPU_LOG")
        );
    }

    #[tokio::test]
    async fn spawns_and_waits_for_a_short_lived_child() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut input = spec();
        input.working_dir = String::from(".");
        #[cfg(unix)]
        {
            input.program = String::from("sh");
            input.args = vec![String::from("-c"), String::from("exit 0")];
        }
        #[cfg(windows)]
        {
            input.program = String::from("cmd");
            input.args = vec![String::from("/c"), String::from("exit 0")];
        }
        let mut process = VmProcess::spawn(&input, directory.path()).expect("spawn");
        let status = process
            .wait_within(Duration::from_secs(5))
            .await
            .expect("wait")
            .expect("status");
        assert!(status.success());
    }
}
