# Source for fallow-demo.gif. Regenerate from the repository root:
#   cargo build --release -p fallow-cli --bin fallow
#   vhs assets/screenshots/demo.tape
# The hidden setup copies assets/demo-project to a temporary directory,
# installs its dependencies, and routes `npx fallow` to the local release
# build, so the recording shows the checked-out version with zero config.

Output assets/screenshots/fallow-demo.gif

Set Shell "bash"
Set FontFamily "Victor Mono"
Set FontSize 15
Set Width 1200
Set Height 940
Set Padding 24
Set Theme "Catppuccin Frappe"
Set TypingSpeed 55ms
Set WindowBar Colorful
Set BorderRadius 10

Hide
Type `export FALLOW_BIN="$PWD/target/release/fallow" FALLOW_TELEMETRY=off FALLOW_UPDATE_CHECK=off DEMO="${TMPDIR:-/tmp}/fallow-demo/my-app"`
Enter
Type `rm -rf "$DEMO" && mkdir -p "$DEMO" && cp -R assets/demo-project/package.json assets/demo-project/src "$DEMO" && cd "$DEMO"`
Enter
Type "npm install --silent --no-audit --no-fund >/dev/null 2>&1 && git init -q && git add -A && git -c user.name=demo -c user.email=demo@example.com commit -qm init"
Enter
Wait@120s
Type `npx() { shift; "$FALLOW_BIN" "$@"; }; PS1='\[\e[1;36m\]my-app\[\e[0m\] ❯ '; clear`
Enter
Show

Sleep 800ms
Type "npx fallow health --score"
Sleep 400ms
Enter
Sleep 3500ms

Type "npx fallow dupes"
Sleep 400ms
Enter
Sleep 3500ms

Type "clear"
Enter
Type "npx fallow --summary"
Sleep 400ms
Enter
Sleep 6s
