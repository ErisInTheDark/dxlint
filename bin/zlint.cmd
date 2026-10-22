:<<"::CMDLITERAL"
@echo off
rem Windows: runs the newest train pin (target\bin), else target\release.
rem Both find std\ and rules\ in this repository, their nearest ancestor.
setlocal
set "root=%~dp0.."
set "exe="
for /f "delims=" %%f in ('dir /b /o-d "%root%\target\bin\*-train-*.exe" 2^>nul') do if not defined exe set "exe=%root%\target\bin\%%f"
if not defined exe if exist "%root%\target\release\zlint.exe" set "exe=%root%\target\release\zlint.exe"
if not defined exe (
  echo zlint: no build found; run `cargo build --release` in %root% 1>&2
  exit /b 2
)
"%exe%" %*
exit /b %errorlevel%
::CMDLITERAL
# POSIX sh: the same choice. npm links this file without a shebang, so
# shells run it with sh.
self=$0
while [ -L "$self" ]; do
  link=$(readlink "$self")
  case $link in
    /*) self=$link ;;
    *) self=$(dirname "$self")/$link ;;
  esac
done
root=$(cd "$(dirname "$self")/.." && pwd)
exe=
for file in $(ls -t "$root"/target/bin/ 2>/dev/null); do
  case $file in *-train-*.json|*-train-*.pdb) continue ;; *-train-*) ;; *) continue ;; esac
  if [ -x "$root/target/bin/$file" ]; then exe=$root/target/bin/$file; break; fi
done
[ -n "$exe" ] || exe=$root/target/release/zlint
if [ ! -x "$exe" ]; then
  echo "zlint: no build found; run \`cargo build --release\` in $root" >&2
  exit 2
fi
exec "$exe" "$@"
