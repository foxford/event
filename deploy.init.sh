#!/usr/bin/env bash

if [[ ! ${GITHUB_TOKEN} ]]; then echo "GITHUB_TOKEN is required" 1>&2; exit 1; fi

PROJECT="${PROJECT:-event}"
SOURCE=${SOURCE:-"https://api.github.com/repos/foxford/ulms-env/contents/k8s"}
APPS_SOURCE="https://api.github.com/repos/foxford/ulms-env/contents/apps"
BRANCH="${BRANCH:-master}"
FLAGS="-sSL"

function FILE_FROM_GITHUB() {
    local DEST_DIR="${1}"; if [[ ! "${DEST_DIR}" ]]; then echo "${FUNCNAME[0]}:DEST_DIR is required" 1>&2; exit 1; fi
    local URI="${2}"; if [[ ! "${URI}" ]]; then echo "${FUNCNAME[0]}:URI is required" 1>&2; exit 1; fi
    if [[ "${3}" != "optional" ]]; then
        local FLAGS="-fsSL"
    else
        local FLAGS="-sSL"
    fi

    mkdir -p "${DEST_DIR}"
    curl ${FLAGS} \
        -H "authorization: token ${GITHUB_TOKEN}" \
        -H 'accept: application/vnd.github.v3.raw' \
        -o "${DEST_DIR}/$(basename $URI)" \
        "${URI}?ref=${BRANCH}"
}

function DIR_FROM_GITHUB() {
    local FILES="${1}"; if [[ ! "${FILES}" ]]; then echo "${FUNCNAME[0]}:FILES is required" 1>&2; exit 1; fi
    local DEST_DIR="${2}"; if [[ ! "${DEST_DIR}" ]]; then echo "${FUNCNAME[0]}:DEST_DIR is required" 1>&2; exit 1; fi

    for FILE in $FILES
    do
        FILE=$(echo $FILE | sed -e "s/?.*//")
        FILE_FROM_GITHUB ${DEST_DIR} ${FILE}
    done
}

function LIST_GITHUB_DIR() {
    local URI="${1}"; if [[ ! "${URI}" ]]; then echo "${FUNCNAME[0]}:URI is required" 1>&2; exit 1; fi

    curl ${FLAGS} \
        -H "authorization: token ${GITHUB_TOKEN}" \
        -H 'accept: application/vnd.github.v3.raw' \
        $URI
}

function ADD_PROJECT() {
    local _PATH="${1}"; if [[ ! "${_PATH}" ]]; then echo "${FUNCNAME[0]}:_PATH is required" 1>&2; exit 1; fi
    local _PROJECT="${2}"; if [[ ! "${_PROJECT}" ]]; then echo "${FUNCNAME[0]}:PROJECT is required" 1>&2; exit 1; fi

    # insert PROJECT=${_PROJECT} as second line under a shebang
    cat ${_PATH} | awk "NR==1{print; print \"PROJECT=${_PROJECT}\"} NR!=1" > ${_PATH}.tmp
    mv ${_PATH}.tmp ${_PATH}
}

function DIR_FROM_GITHUB_RECURSIVELY() {
    local SRC_SUBDIR="${1}"
    if [[ ! "${SRC_SUBDIR}" ]]; then echo "${FUNCNAME[0]}:SRC_SUBDIR is required" 1>&2; exit 1; fi
    local DEST_SUBDIR="${2}"
    if [[ ! "${DEST_SUBDIR}" ]]; then echo "${FUNCNAME[0]}:DEST_SUBDIR is required" 1>&2; exit 1; fi

    mkdir -p "deploy/k8s/$DEST_SUBDIR"
    CONTENT=$(LIST_GITHUB_DIR "${SOURCE}/apps/deploy/${PROJECT}/${SRC_SUBDIR}?ref=${BRANCH}")

    FILES=$(echo $CONTENT | jq '.[] | select(.type == "file") | .download_url' -r)
    DIR_FROM_GITHUB "${FILES}" "deploy/k8s/${DEST_SUBDIR}"

    DIRS=$(echo $CONTENT | jq '.[] | select(.type == "dir") | .url' -r)

    for DIR in $DIRS
    do
        DIR=$(echo $DIR | sed -e "s/?.*//")

        DIR_CONTENT=$(LIST_GITHUB_DIR "${DIR}?ref=${BRANCH}")

        mkdir -p "deploy/k8s/${DEST_SUBDIR}/$(basename $DIR)"

        FILES=$(echo $DIR_CONTENT | jq '.[] | select(.type == "file") | .download_url' -r)
        DIR_FROM_GITHUB "${FILES}" "deploy/k8s/${DEST_SUBDIR}/$(basename $DIR)"
    done
}

set -ex

if [[ -n ${NAMESPACE} ]]; then
     FILE_FROM_GITHUB "deploy" "${SOURCE}/certs/ca-${NAMESPACE}.crt"

    SHORT_NS=$(echo $NAMESPACE | sed s/-ng/-foxford/ | sed -E "s/^(.)([[:alpha:]]*)(.*)$/\1\3/")
    FILE_FROM_GITHUB "deploy" "${APPS_SOURCE}/${SHORT_NS}/${PROJECT}/values.yaml"

    echo "In order to enable deployment NAMESPACE is required."
fi

## Get dependencies.
FILE_FROM_GITHUB "deploy" "${SOURCE}/utils/ci-install-tools.sh"

## Use the same project for build & deploy scripts.
CI_FILES=(ci-build.sh ci-deploy.sh ci-mdbook.sh github-actions-run.sh)
for FILE in ${CI_FILES[@]}; do
    FILE_FROM_GITHUB "deploy" "${SOURCE}/utils/${FILE}"
    ADD_PROJECT "deploy/${FILE}" "${PROJECT}"
done

chmod u+x deploy/{ci-mdbook.sh,ci-build.sh,ci-deploy.sh,ci-install-tools.sh,github-actions-run.sh}
