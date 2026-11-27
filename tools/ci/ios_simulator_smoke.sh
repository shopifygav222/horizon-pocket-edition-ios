#!/bin/bash
# The app's start in the iOS Simulator, checked before any phone sees it (IOS-SYNC.md): it is installed
# in a fresh iPhone simulator and launched, and after a while its logs must show Starling's own start
# (its log opened, the iOS layer's start line, the renderer starting or saying why it can't), with no
# panic in stderr and no crash report. GitHub's macOS machines may give the Simulator no Metal device:
# then the renderer's "No Metal graphics device" is what the start reaches, which still proves every
# step before it.
#   tools/ci/ios_simulator_smoke.sh path/to/HorizonPE.app [CATALOG_MODS]
# With CATALOG_MODS (`id@version,id@version`, the mods the app was built with:
# tools/ios_native_sdk.py), the start must also say it read each mod's package from the bundle.
set -euo pipefail
app=$1
catalog_mods=${2:-}
bundle=dev.horizon.pocketedition
device=$(xcrun simctl list devices available -j | python3 -c '
import json, sys
devices = json.load(sys.stdin)["devices"]
phones = [d for runtime, list_ in devices.items() if "iOS" in runtime for d in list_ if d["name"].startswith("iPhone")]
print(phones[-1]["udid"] if phones else "")
')
test -n "$device" || { echo '::error::no iPhone simulator on this machine'; exit 1; }
echo "Simulator: $device"
xcrun simctl boot "$device" || true
xcrun simctl bootstatus "$device" -b
xcrun simctl install "$device" "$app"
started=$(date +%s)
xcrun simctl launch "$device" "$bundle"
sleep 120
data=$(xcrun simctl get_app_container "$device" "$bundle" data)
logs="$data/Documents/logs"
echo "::group::stderr.log"; cat "$logs/stderr.log" 2>/dev/null || true; echo "::endgroup::"
echo "::group::latest.log"; cat "$logs/latest.log" 2>/dev/null || true; echo "::endgroup::"
failed=0
# The Simulator's GPU is not of Apple's family 4, so wgpu gives it 15 inter-stage shader variables
# where every iPhone iOS 16 runs on (A11 and later) has 31: the renderer's pipelines stop there,
# in the Simulator only. Any other panic is the app's.
simulator_limit='max_inter_stage_shader_variables` limit (15)'
simulator_stop=0
if grep -qF "$simulator_limit" "$logs/stderr.log" 2>/dev/null; then
  simulator_stop=1
  echo "The Simulator's own shader-variable limit stopped the renderer's pipelines, as expected there."
elif grep -q 'panicked at' "$logs/stderr.log" 2>/dev/null; then
  echo '::error::the app panicked in the Simulator (stderr.log above)'
  failed=1
fi
# The app here has no game files (they are fetched on the phone), so from the 0.9.0 engine on the
# start ends on the setup screen, drawn by the loading screen's own small renderer: its first
# frame is as far as it goes. (With game files the game's renderer follows, and stops at the
# Simulator's limit above.)
if ! grep -qE 'Horizon: loading screen shown|Horizon renderer: creating the pipelines' "$logs/latest.log" 2>/dev/null; then
  echo "::error::the game did not draw its loading screen (latest.log above)"
  failed=1
fi
if ! grep -q 'Horizon renderer: device ready' "$logs/latest.log" 2>/dev/null; then
  echo "::error::the renderer got no device (latest.log above)"
  failed=1
fi
if ! grep -qE 'Horizon: iOS, game folder' "$logs/latest.log" 2>/dev/null; then
  echo "::error::Starling's start did not reach its log (latest.log above)"
  failed=1
fi
if [ -n "$catalog_mods" ]; then
  if grep -q 'the mods this app was built with are left out' "$logs/stderr.log" 2>/dev/null; then
    echo '::error::the app left out the mods it was built with (stderr.log above)'
    failed=1
  fi
  built=$(grep -F 'Horizon: mods built into this app:' "$logs/stderr.log" 2>/dev/null || true)
  for mod in ${catalog_mods//,/ }; do
    if case "$built" in *"${mod%%@*} ${mod#*@}"*) false ;; *) true ;; esac; then
      echo "::error::the app did not say it was built with $mod (stderr.log above)"
      failed=1
    fi
  done
fi
crashes=$(find "$HOME/Library/Logs/DiagnosticReports" -newermt "@$started" -name 'Starling*' 2>/dev/null || true)
if [ -n "$crashes" ]; then
  for report in $crashes; do echo "::group::$report"; head -120 "$report"; echo "::endgroup::"; done
  # The Simulator-only stop above ends the app too.
  if [ $simulator_stop = 0 ]; then
    echo '::error::the app crashed in the Simulator (reports above)'
    failed=1
  fi
fi
# (Listed whole, then searched without a pipe: a search that stops at its first match closes the
# pipe on whatever writes to it, and that "Broken pipe" was read here as the app having left.)
services=$(xcrun simctl spawn "$device" launchctl list 2>/dev/null || true)
if case "$services" in *"UIKitApplication:$bundle"*) true ;; *) false ;; esac; then
  echo 'The app is still running.'
elif [ $simulator_stop = 1 ]; then
  echo "The app stopped at the Simulator's limit."
else
  # Two minutes in, a start that got this far and then left is a crash or an exit nobody asked for.
  echo '::error::the app is no longer running'
  failed=1
fi
xcrun simctl shutdown "$device" || true
exit $failed
