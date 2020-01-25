#!/bin/sh
set -xe

sql-migrate up -config=/dbconfig.yml
api
