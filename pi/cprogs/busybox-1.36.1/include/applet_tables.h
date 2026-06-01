/* This is a generated file, don't edit */

#define NUM_APPLETS 10
#define KNOWN_APPNAME_OFFSETS 0

const char applet_names[] ALIGN1 = ""
"ash" "\0"
"cat" "\0"
"cp" "\0"
"crc32" "\0"
"env" "\0"
"fatattr" "\0"
"ls" "\0"
"mkdir" "\0"
"sh" "\0"
"vi" "\0"
;

#define APPLET_NO_ash 0
#define APPLET_NO_cat 1
#define APPLET_NO_cp 2
#define APPLET_NO_crc32 3
#define APPLET_NO_env 4
#define APPLET_NO_fatattr 5
#define APPLET_NO_ls 6
#define APPLET_NO_mkdir 7
#define APPLET_NO_sh 8
#define APPLET_NO_vi 9

#ifndef SKIP_applet_main
int (*const applet_main[])(int argc, char **argv) = {
ash_main,
cat_main,
cp_main,
cksum_main,
env_main,
fatattr_main,
ls_main,
mkdir_main,
ash_main,
vi_main,
};
#endif

const uint8_t applet_flags[] ALIGN1 = {
0xa0,
0xea,
0x00,
};

