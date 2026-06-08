set -ex

main() {
    rustup self update
    local target=
    if [ $TRAVIS_OS_NAME = linux ]; then
        target=x86_64-unknown-linux-musl
        sort=sort
    else
        target=x86_64-apple-darwin
        sort=gsort  # for `sort --sort-version`, from brew's coreutils.
    fi

    # This fetches latest stable release
    local tag=$(git ls-remote --tags --refs --exit-code https://github.com/japaric/cross \
                       | cut -d/ -f3 \
                       | grep -E '^v[0.1.0-9.]+$' \
                       | $sort --version-sort \
                       | tail -n1)
    curl -LSfs https://japaric.github.io/trust/install.sh | \
        sh -s -- \
           --force \
           --git japaric/cross \
           --tag $tag \
           --target $target

    MYSQLSH_DL_LINK=""
    if [ $TRAVIS_OS_NAME = linux ]; then
        MYSQLSH_DL_LINK="https://dev.mysql.com/get/Downloads/MySQL-Shell/mysql-shell-1.0.11-linux-glibc2.12-x86-64bit.tar.gz"
        if [[ $TARGET == i686* ]]; then
            MYSQLSH_DL_LINK="https://dev.mysql.com/get/Downloads/MySQL-Shell/mysql-shell-1.0.11-linux-glibc2.12-x86-32bit.tar.gz"
        fi
    else
        MYSQLSH_DL_LINK="https://dev.mysql.com/get/Downloads/MySQL-Shell/mysql-shell-1.0.11-macos10.13-x86-64bit.tar.gz"
    fi
    curl -LO $MYSQLSH_DL_LINK
    tar -xf mysql-shell*.tar.gz
    rm -f mysql-shell*.tar.gz
    mkdir -p /usr/local/bin
    sudo mv mysql-shell*bit/bin/mysqlsh /usr/local/bin/
}

main
