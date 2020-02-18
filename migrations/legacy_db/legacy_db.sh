#!/bin/bash

function REPORT_ERROR() { >&2 echo ${@}; }

if [[ ! ${SOURCE_DB} ]]; then $(REPORT_ERROR "SOURCE_DB isn't specified"); exit 1; fi
if [[ ! ${TARGET_DB} ]]; then $(REPORT_ERROR "TARGET_DB isn't specified"); exit 1; fi

PSQL=${PSQL:-'psql'}

SOURCE_HOST=${SOURCE_HOST:-'127.0.0.1'}
SOURCE_PORT=${SOURCE_PORT:-'5432'}
SOURCE_USER=${SOURCE_USER:-'postgres'}
SOURCE_PASSWORD=${SOURCE_PASSWORD:-''}

TARGET_HOST=${TARGET_HOST:-'127.0.0.1'}
TARGET_PORT=${TARGET_PORT:-'5432'}
TARGET_USER=${TARGET_USER:-'postgres'}
TARGET_PASSWORD=${TARGET_PASSWORD:-''}

set -xe

cat ./legacy_db.sql |
  envsubst |
  PGPASSWORD=${TARGET_PASSWORD} ${PSQL} -h ${TARGET_HOST} -p ${TARGET_PORT} -U ${TARGET_USER} ${TARGET_DB}
