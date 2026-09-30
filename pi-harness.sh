#!@shell@
if [ "${EKKO_INSTANCE-}" = pi-harness ] && [ $# -eq 0 ]; then set -- new; fi
export EKKO_INSTANCE=pi-harness EKKO_CONFIG=@profile@
case "${1-}" in
  "") set -- run @env@ -C "$PWD" @pi@ -e @status@ ;;
  new) set -- command new "$(cd -- "${2:-.}" && pwd)" ;;
esac
exec @ekko@ --bare "$@"
