make allnoconfig

# Core BusyBox
sed -i 's/# CONFIG_BUSYBOX is not set/CONFIG_BUSYBOX=y/' .config
sed -i 's/# CONFIG_STATIC is not set/CONFIG_STATIC=y/' .config

# Shell: use ASH only
sed -i 's/# CONFIG_SH_IS_ASH is not set/CONFIG_SH_IS_ASH=y/' .config
sed -i 's/# CONFIG_SHELL_IS_ASH is not set/CONFIG_SHELL_IS_ASH=y/' .config
sed -i 's/# CONFIG_ASH is not set/CONFIG_ASH=y/' .config

# Minimal ash features
sed -i 's/# CONFIG_ASH_ECHO is not set/CONFIG_ASH_ECHO=y/' .config
sed -i 's/# CONFIG_ASH_ECHO is not set/CONFIG_ASH_ECHO=y/' .config
sed -i 's/# CONFIG_ASH_BUILTIN_ECHO is not set/CONFIG_ASH_BUILTIN_ECHO=y/' .config

# Disable features that require more complete kernel
sed -i 's/CONFIG_ASH_JOB_CONTROL=y/# CONFIG_ASH_JOB_CONTROL is not set/' .config
sed -i 's/CONFIG_FEATURE_EDITING=y/# CONFIG_FEATURE_EDITING is not set/' .config 
sed -i 's/CONFIG_FEATURE_EDITING_HISTORY=y/# CONFIG_FEATURE_EDITING_HISTORY is not set/' .config 
sed -i 's/CONFIG_FEATURE_TAB_COMPLETION=y/# CONFIG_FEATURE_TAB_COMPLETION is not set/' .config 

# Minimal applets
sed -i 's/# CONFIG_SH is not set/CONFIG_SH=y/' .config
sed -i 's/# CONFIG_LS is not set/CONFIG_LS=y/' .config
sed -i 's/# CONFIG_CAT is not set/CONFIG_CAT=y/' .config
sed -i 's/# CONFIG_ECHO is not set/CONFIG_ECHO=y/' .config
sed -i 's/# CONFIG_PWD is not set/CONFIG_PWD=y/' .config
sed -i 's/# CONFIG_TRUE is not set/CONFIG_TRUE=y/' .config
sed -i 's/# CONFIG_FALSE is not set/CONFIG_FALSE=y/' .config
sed -i 's/# CONFIG_MKDIR is not set/CONFIG_MKDIR=y/' .config

# Reduce shell complexity
sed -i 's/# CONFIG_FEATURE_PREFER_APPLETS is not set/CONFIG_FEATURE_PREFER_APPLETS=y/' .config 
sed -i 's/# CONFIG_ENV is not set/CONFIG_ENV=n/' .config 
sed -i 's/# CONFIG_HUSH is not set/CONFIG_HUSH=n/' .config 

# Search current directory for executables
sed -i 's/# CONFIG_FEATURE_SH_STANDALONE is not set/CONFIG_FEATURE_SH_STANDALONE=y/' .config
sed -i 's/# CONFIG_FEATURE_SH_NOFORK is not set/CONFIG_FEATURE_SH_NOFORK=y/' .config

# It won't compile otherwise
sed -i 's|# CONFIG_LFS is not set|CONFIG_LFS=y|' .config

# Ensure config consistency
yes "" | make oldconfig

make -j4