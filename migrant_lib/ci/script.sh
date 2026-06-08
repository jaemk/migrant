# This script takes care of testing your crate

set -ex

# TODO This is the "test phase", tweak it as you see fit
main() {
    rustup self update
    #cross build --target $TARGET
    #cross build --target $TARGET --release

    #if [ ! -z $DISABLE_TESTS ]; then
    #    return
    #fi

    #cross test --target $TARGET
    if [ $TRAVIS_OS_NAME = linux ]; then
        ./test.sh --no-pass
    fi
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
