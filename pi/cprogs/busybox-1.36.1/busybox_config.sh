make allnoconfig
sed -i 's|# CONFIG_STATIC is not set|CONFIG_STATIC=y|' .config
sed -i 's|# CONFIG_LFS is not set|CONFIG_LFS=y|' .config
sed -i 's|# CONFIG_BUSYBOX is not set|CONFIG_BUSYBOX=y|' .config
sed -i 's|# CONFIG_SH_IS_ASH is not set|CONFIG_SH_IS_ASH=y|' .config
sed -i 's|# CONFIG_SHELL_ASH is not set|CONFIG_SHELL_ASH=y|' .config
sed -i 's|# CONFIG_ASH is not set|CONFIG_ASH=y|' .config
