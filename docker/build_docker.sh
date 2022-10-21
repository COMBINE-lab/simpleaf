#! /bin/bash
USEFULAF_VERSION=0.6.0
docker build --no-cache -t combinelab/usefulaf:${USEFULAF_VERSION} -t combinelab/usefulaf:latest .
