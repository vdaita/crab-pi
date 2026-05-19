make allnoconfig
sed -i 's|# CONFIG_STATIC is not set|CONFIG_STATIC=y|' .config
sed -i 's|# CONFIG_LFS is not set|CONFIG_LFS=y|' .config
sed -i 's|# CONFIG_BUSYBOX is not set|CONFIG_BUSYBOX=y|' .config
sed -i 's|# CONFIG_SH_IS_ASH is not set|CONFIG_SH_IS_ASH=y|' .config
sed -i 's|# CONFIG_SHELL_ASH is not set|CONFIG_SHELL_ASH=y|' .config
sed -i 's|# CONFIG_ASH is not set|CONFIG_ASH=y|' .config
sed -i 's|# CONFIG_ASH_ECHO is not set|CONFIG_ASH_ECHO=y|' .config
sed -i 's|# CONFIG_FATATTR is not set|CONFIG_FATATTR=y|' .config
sed -i 's|# CONFIG_LS is not set|CONFIG_LS=y|' .config
sed -i 's|# CONFIG_CAT is not set|CONFIG_CAT=y|' .config
sed -i 's|# CONFIG_VI is not set|CONFIG_VI=y|' .config