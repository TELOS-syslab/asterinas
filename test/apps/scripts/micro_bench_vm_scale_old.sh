#!/bin/sh

set -ex

/test/scale_old/mmap_pf
/test/scale_old/mmap
/test/scale_old/munmap_dist
/test/scale_old/munmap_virt
/test/scale_old/pf_dist
/test/scale_old/pf_rand

poweroff -f
