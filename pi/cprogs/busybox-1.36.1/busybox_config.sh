make allnoconfig
sed -i 's|# CONFIG_STATIC is not set|CONFIG_STATIC=y|' .config
sed -i 's|# CONFIG_LFS is not set|CONFIG_LFS=y|' .config
sed -i 's|# CONFIG_BUSYBOX is not set|CONFIG_BUSYBOX=y|' .config
sed -i 's|# CONFIG_FEATURE_PREFER_APPLETS is not set|CONFIG_FEATURE_PREFER_APPLETS=y|' .config
sed -i 's|# CONFIG_FEATURE_SH_STANDALONE is not set|CONFIG_FEATURE_SH_STANDALONE=y|' .config
sed -i 's|# CONFIG_FEATURE_SH_NOFORK is not set|CONFIG_FEATURE_SH_NOFORK=y|' .config
sed -i 's|# CONFIG_DEBUG is not set|CONFIG_DEBUG=y|' .config
sed -i 's|# CONFIG_ASH_PRINTF is not set|CONFIG_ASH_PRINTF=y|' .config
# sed -i 's|# CONFIG_ASH_JOB_CONTROL is not set|CONFIG_ASH_JOB_CONTROL=y|' .config

# Disable ASH completely
# sed -i 's/^CONFIG_ASH=y/# CONFIG_ASH is not set/' .config
# sed -i 's/^CONFIG_SH_IS_ASH=y/# CONFIG_SH_IS_ASH is not set/' .config
# sed -i 's/^CONFIG_SHELL_ASH=y/# CONFIG_SHELL_ASH is not set/' .config

# Enable HUSH and its standalone capabilities
# sed -i 's/^# CONFIG_HUSH is not set/CONFIG_HUSH=y/' .config
# sed -i 's/^# CONFIG_SH_IS_HUSH is not set/CONFIG_SH_IS_HUSH=y/' .config
# sed -i 's/^# CONFIG_SHELL_HUSH is not set/CONFIG_SHELL_HUSH=y/' .config
# sed -i 's/^# CONFIG_HUSH_STANDALONE is not set/CONFIG_HUSH_STANDALONE=y/' .config
# sed -i 's/^# CONFIG_HUSH_FUNCTIONS is not set/CONFIG_HUSH_FUNCTIONS=y/' .config
# sed -i \
#   -e 's/^# CONFIG_HUSH_BASH_COMPAT is not set/CONFIG_HUSH_BASH_COMPAT=y/' \
#   -e 's/^# CONFIG_HUSH_BRACE_EXPANSION is not set/CONFIG_HUSH_BRACE_EXPANSION=y/' \
#   -e 's/^# CONFIG_HUSH_BASH_SOURCE_CURDIR is not set/CONFIG_HUSH_BASH_SOURCE_CURDIR=y/' \
#   -e 's/^# CONFIG_HUSH_LINENO_VAR is not set/CONFIG_HUSH_LINENO_VAR=y/' \
#   -e 's/^# CONFIG_HUSH_INTERACTIVE is not set/CONFIG_HUSH_INTERACTIVE=y/' \
#   -e 's/^# CONFIG_HUSH_SAVEHISTORY is not set/CONFIG_HUSH_SAVEHISTORY=y/' \
#   -e 's/^# CONFIG_HUSH_JOB is not set/CONFIG_HUSH_JOB=y/' \
#   -e 's/^# CONFIG_HUSH_TICK is not set/CONFIG_HUSH_TICK=y/' \
#   -e 's/^# CONFIG_HUSH_IF is not set/CONFIG_HUSH_IF=y/' \
#   -e 's/^# CONFIG_HUSH_LOOPS is not set/CONFIG_HUSH_LOOPS=y/' \
#   -e 's/^# CONFIG_HUSH_CASE is not set/CONFIG_HUSH_CASE=y/' \
#   -e 's/^# CONFIG_HUSH_FUNCTIONS is not set/CONFIG_HUSH_FUNCTIONS=y/' \
#   -e 's/^# CONFIG_HUSH_LOCAL is not set/CONFIG_HUSH_LOCAL=y/' \
#   -e 's/^# CONFIG_HUSH_RANDOM_SUPPORT is not set/CONFIG_HUSH_RANDOM_SUPPORT=y/' \
#   -e 's/^# CONFIG_HUSH_MODE_X is not set/CONFIG_HUSH_MODE_X=y/' \
#   -e 's/^# CONFIG_HUSH_ECHO is not set/CONFIG_HUSH_ECHO=y/' \
#   -e 's/^# CONFIG_HUSH_PRINTF is not set/CONFIG_HUSH_PRINTF=y/' \
#   -e 's/^# CONFIG_HUSH_TEST is not set/CONFIG_HUSH_TEST=y/' \
#   -e 's/^# CONFIG_HUSH_HELP is not set/CONFIG_HUSH_HELP=y/' \
#   -e 's/^# CONFIG_HUSH_EXPORT is not set/CONFIG_HUSH_EXPORT=y/' \
#   -e 's/^# CONFIG_HUSH_EXPORT_N is not set/CONFIG_HUSH_EXPORT_N=y/' \
#   -e 's/^# CONFIG_HUSH_READONLY is not set/CONFIG_HUSH_READONLY=y/' \
#   -e 's/^# CONFIG_HUSH_KILL is not set/CONFIG_HUSH_KILL=y/' \
#   -e 's/^# CONFIG_HUSH_WAIT is not set/CONFIG_HUSH_WAIT=y/' \
#   -e 's/^# CONFIG_HUSH_COMMAND is not set/CONFIG_HUSH_COMMAND=y/' \
#   -e 's/^# CONFIG_HUSH_TRAP is not set/CONFIG_HUSH_TRAP=y/' \
#   -e 's/^# CONFIG_HUSH_TYPE is not set/CONFIG_HUSH_TYPE=y/' \
#   -e 's/^# CONFIG_HUSH_TIMES is not set/CONFIG_HUSH_TIMES=y/' \
#   -e 's/^# CONFIG_HUSH_READ is not set/CONFIG_HUSH_READ=y/' \
#   -e 's/^# CONFIG_HUSH_SET is not set/CONFIG_HUSH_SET=y/' \
#   -e 's/^# CONFIG_HUSH_UNSET is not set/CONFIG_HUSH_UNSET=y/' \
#   -e 's/^# CONFIG_HUSH_ULIMIT is not set/CONFIG_HUSH_ULIMIT=y/' \
#   -e 's/^# CONFIG_HUSH_UMASK is not set/CONFIG_HUSH_UMASK=y/' \
#   -e 's/^# CONFIG_HUSH_GETOPTS is not set/CONFIG_HUSH_GETOPTS=y/' \
#   -e 's/^# CONFIG_HUSH_MEMLEAK is not set/CONFIG_HUSH_MEMLEAK=y/' \
# .config

sed -i 's|# CONFIG_SH_IS_ASH is not set|CONFIG_SH_IS_ASH=y|' .config
sed -i 's|# CONFIG_SHELL_ASH is not set|CONFIG_SHELL_ASH=y|' .config
sed -i 's|# CONFIG_ASH is not set|CONFIG_ASH=y|' .config
sed -i 's|# CONFIG_ASH_ECHO is not set|CONFIG_ASH_ECHO=y|' .config

sed -i 's|# CONFIG_FATATTR is not set|CONFIG_FATATTR=y|' .config
sed -i 's|# CONFIG_LS is not set|CONFIG_LS=y|' .config
sed -i 's|# CONFIG_CAT is not set|CONFIG_CAT=y|' .config
sed -i 's|# CONFIG_VI is not set|CONFIG_VI=y|' .config