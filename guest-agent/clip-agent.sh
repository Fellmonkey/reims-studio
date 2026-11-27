#!/bin/zsh
# Guest-side clipboard relay for Reims Studio.
# The launcher writes the bridge token into the LaunchAgent before first boot.

token="${REIMS_BRIDGE_TOKEN:?REIMS_BRIDGE_TOKEN is required}"
port="${REIMS_BRIDGE_PORT:-8080}"
url="http://10.222.0.2:${port}"
last=""

while true; do
  remote="$(curl -fsS -H "Authorization: Bearer ${token}" "${url}/clipboard" 2>/dev/null || true)"
  local="$(pbpaste 2>/dev/null || true)"

  if [ -n "${remote}" ] && [ "${remote}" != "${last}" ] && [ "${remote}" != "${local}" ]; then
    printf '%s' "${remote}" | pbcopy
    last="${remote}"
  fi

  if [ "${local}" != "${last}" ]; then
    printf '%s' "${local}" | curl -fsS -X POST \
      -H "Authorization: Bearer ${token}" \
      --data-binary @- "${url}/clipboard" >/dev/null 2>&1 || true
    last="${local}"
  fi

  sleep 1
done
