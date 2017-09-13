# This script takes care of building your crate and packaging it for release

set -ex

main() {
    rustup self update
    local src=$(pwd) \
          stage=

    case $TRAVIS_OS_NAME in
        linux)
            stage=$(mktemp -d)
            ;;
        osx)
            stage=$(mktemp -d -t tmp)
            ;;
    esac

    test -f Cargo.lock || cargo generate-lockfile

    # TODO Update this to build the artifacts that matter to you
    cross rustc --bin migrant --target $TARGET --release --features 'postgresql sqlite update' -- -C lto

    # TODO Update this to package the right artifacts
    cp target/$TARGET/release/migrant $stage/

    cd $stage
    tar czf $src/$CRATE_NAME-$TRAVIS_TAG-$TARGET.tar.gz *
    cd $src

    rm -rf $stage
}

main
