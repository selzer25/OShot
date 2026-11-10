#!/bin/sh
# Builds dist/OShot.app and signs it.
#
# macOS ties the Screen Recording permission to the app's code signature. An
# ad-hoc signature changes on every build, which silently revokes the
# permission, so this signs with a real identity (first "Apple Development"
# certificate in the keychain unless OSHOT_SIGN_IDENTITY is set). Without
# a certificate, OSHOT_SIGN_IDENTITY=- signs ad hoc.
set -eu

cd "$(dirname "$0")/.."

cargo build --release -p oshot-app

identity="${OSHOT_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Apple Development[^"]*\)".*/\1/p' | head -n 1)}"
if [ -z "$identity" ]; then
    echo "No signing identity found. Set OSHOT_SIGN_IDENTITY, or OSHOT_SIGN_IDENTITY=- to sign ad hoc." >&2
    exit 1
fi

app="dist/OShot.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/oshot "$app/Contents/MacOS/oshot"
cp bundle/Info.plist "$app/Contents/Info.plist"
cp bundle/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

codesign --force --options runtime --sign "$identity" "$app"
echo "Built $app (signed by $identity)"
