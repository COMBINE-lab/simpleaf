#! /bin/bash
USEFULAF_VERSION=0.8.1
TMPDIR=/mnt/scratch2/DELETE_ME_TEMP docker build --no-cache -t combinelab/usefulaf:${USEFULAF_VERSION} -t combinelab/usefulaf:latest .
