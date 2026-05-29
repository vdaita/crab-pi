/* This is a generated file, don't edit */

#define NUM_APPLETS 16
#define KNOWN_APPNAME_OFFSETS 0

const char applet_names[] ALIGN1 = ""
"ash" "\0"
"basename" "\0"
"cat" "\0"
"clear" "\0"
"cmp" "\0"
"cp" "\0"
"crc32" "\0"
"diff" "\0"
"env" "\0"
"fatattr" "\0"
"ls" "\0"
"mkdir" "\0"
"sh" "\0"
"touch" "\0"
"which" "\0"
"yes" "\0"
;

#define APPLET_NO_ash 0
#define APPLET_NO_basename 1
#define APPLET_NO_cat 2
#define APPLET_NO_clear 3
#define APPLET_NO_cmp 4
#define APPLET_NO_cp 5
#define APPLET_NO_crc32 6
#define APPLET_NO_diff 7
#define APPLET_NO_env 8
#define APPLET_NO_fatattr 9
#define APPLET_NO_ls 10
#define APPLET_NO_mkdir 11
#define APPLET_NO_sh 12
#define APPLET_NO_touch 13
#define APPLET_NO_which 14
#define APPLET_NO_yes 15

#ifndef SKIP_applet_main
int (*const applet_main[])(int argc, char **argv) = {
ash_main,
basename_main,
cat_main,
clear_main,
cmp_main,
cp_main,
cksum_main,
diff_main,
env_main,
fatattr_main,
ls_main,
mkdir_main,
ash_main,
touch_main,
which_main,
yes_main,
};
#endif

const uint8_t applet_flags[] ALIGN1 = {
0xcc,
0x28,
0xea,
0xbc,
};

