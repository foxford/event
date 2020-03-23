#!/bin/bash

function REPORT_ERROR() { >&2 echo ${@}; }

if [[ ! ${SOURCE_DB} ]]; then $(REPORT_ERROR "SOURCE_DB isn't specified"); exit 1; fi
if [[ ! ${TARGET_DB} ]]; then $(REPORT_ERROR "TARGET_DB isn't specified"); exit 1; fi

SCRIPT=${1:-'primary'}
PSQL=${PSQL:-'psql'}

SOURCE_HOST=${SOURCE_HOST:-'127.0.0.1'}
SOURCE_PORT=${SOURCE_PORT:-'5432'}
SOURCE_USER=${SOURCE_USER:-'postgres'}
SOURCE_PASSWORD=${SOURCE_PASSWORD:-''}

TARGET_HOST=${TARGET_HOST:-'127.0.0.1'}
TARGET_PORT=${TARGET_PORT:-'5432'}
TARGET_USER=${TARGET_USER:-'postgres'}
TARGET_PASSWORD=${TARGET_PASSWORD:-''}

if [ ${SCRIPT} == 'reverse' ]
then
    CONNECT_HOST=${SOURCE_HOST}
    CONNECT_PORT=${SOURCE_PORT}
    CONNECT_USER=${SOURCE_USER}
    CONNECT_PASSWORD=${SOURCE_PASSWORD}
    CONNECT_DB=${SOURCE_DB}
else
    CONNECT_HOST=${TARGET_HOST}
    CONNECT_PORT=${TARGET_PORT}
    CONNECT_USER=${TARGET_USER}
    CONNECT_PASSWORD=${TARGET_PASSWORD}
    CONNECT_DB=${TARGET_DB}
fi

set -xe

echo "Connecting to ${CONNECT_DB}"

cat ./${SCRIPT}.sql |
    envsubst |
    PGPASSWORD=${TARGET_PASSWORD} ${PSQL} \
        -h ${CONNECT_HOST} \
        -p ${CONNECT_PORT} \
        -U ${CONNECT_USER} \
        ${CONNECT_DB}
