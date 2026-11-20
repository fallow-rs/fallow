//! The "Why it matters" text of [`crate::explain`]: what goes wrong for a team
//! that keeps the finding. A rule without an entry here uses its full
//! description.

/// The rationale of a dead-code, health, audit or runtime rule.
const RULE_RATIONALES: &[(&str, &str)] = &[
    (
        "fallow/unused-file",
        "No code runs the file, but people still read it, search it and update it in refactors. A bug fix in the file has no effect on the running program.",
    ),
    (
        "fallow/unused-export",
        "An export that nothing imports looks like an API that other code depends on, so nobody dares to change or remove it. Code that only this export uses stays in the project with it.",
    ),
    (
        "fallow/unused-dev-dependency",
        "Every install on every developer machine and in every CI run downloads the package. Its security advisories and version updates still need a review, for a package that no code uses.",
    ),
    (
        "fallow/unused-optional-dependency",
        "The package manager still tries to install the package on each install. Its advisories and version updates still need a review, and readers assume that some platform needs it.",
    ),
    (
        "fallow/unused-enum-member",
        "Readers treat each enum member as a state that can occur, so switch statements, mappings and tests keep a branch for a value that no code uses.",
    ),
    (
        "fallow/unused-class-member",
        "No code calls the member, but it stays in the public shape of the class. People keep it in step during refactors, and readers assume that some caller depends on it.",
    ),
    (
        "fallow/unused-namespace-member",
        "No code reads the member, but its export makes it look like an API that other code depends on. People keep it in step during refactors and do not dare to remove it.",
    ),
    (
        "fallow/unresolved-import",
        "When the runtime or the bundler cannot resolve the path either, the build fails or the module throws when it loads. fallow also loses the edge, so the files behind it can show up as unused.",
    ),
    (
        "fallow/boundary-coverage",
        "A file outside every zone is outside every boundary rule, so it can import from any layer. New code in that file can break the architecture, and no check reports it.",
    ),
    (
        "fallow/boundary-call-violation",
        "The zone now performs an effect that the architecture keeps out of it, such as a shell command in domain code. Its tests then need that effect, and reviewers of the zone do not expect it.",
    ),
    (
        "fallow/stale-suppression",
        "The comment tells readers that a problem exists where none does. When a new issue of the same kind appears on that line, the old comment hides it from review.",
    ),
    (
        "fallow/missing-suppression-reason",
        "Without a reason, a reviewer cannot tell if the suppression is a decision or a shortcut. Later, nobody can check if the reason still holds, so the suppression stays forever.",
    ),
    (
        "fallow/unused-catalog-entry",
        "No package uses the pinned version, but the entry still gets version updates and reviews. Readers assume that some workspace depends on it.",
    ),
    (
        "fallow/invalid-client-export",
        "Next.js rejects the file when it compiles the route, so the page fails to build. The check reports the export before anyone opens the page or runs the build.",
    ),
    (
        "fallow/absent-component-prop",
        "No caller passes the prop, so the code that reads it always runs with the default value. The other branch has no caller to test it, and the component looks more configurable than it is.",
    ),
    (
        "fallow/unused-component-prop",
        "Callers pass a value that the component ignores, so a caller can think that the prop changes the output. The prop also stays in the component type and in every call site.",
    ),
    (
        "fallow/unused-component-emit",
        "A parent can listen for an event that the component never emits. The handler of the parent never runs, and no error tells anyone.",
    ),
    (
        "fallow/unused-component-output",
        "A parent can bind to an output that the component never emits. The handler of the parent never runs, and no error tells anyone.",
    ),
    (
        "fallow/unused-svelte-event",
        "The component dispatches an event that no parent handles, so the action that the event reports has no effect. Readers assume that some parent reacts to it.",
    ),
    (
        "fallow/unused-tagged-error",
        "Types and `catchTag` handlers plan for a failure that no code produces. The handlers are dead code, and readers think that the failure can occur.",
    ),
    (
        "fallow/long-file",
        "A long file is slow to review and to search. More changes touch the same file, so merge conflicts happen more often.",
    ),
    (
        "fallow/css-token-drift",
        "A hardcoded value does not follow a change of the design token. A theme or brand update misses this place, and the interface becomes inconsistent.",
    ),
    (
        "fallow/css-selector-complexity",
        "A selector with high specificity or `!important` wins over later rules, so the next style change needs a stronger selector. A long descendant chain also breaks when the markup changes.",
    ),
    (
        "fallow/css-dead-surface",
        "The unused style still ships to users and still gets updated in redesigns. A dead scoped class often means that the markup changed and the style stayed behind.",
    ),
    (
        "fallow/css-broken-reference",
        "The class or animation name has no definition, so the element gets no style or no animation. The browser reports no error, so the defect shows only on screen.",
    ),
    (
        "fallow/untested-file",
        "No test imports the file, directly or through other modules. A change to the file can break production, and no test fails.",
    ),
    (
        "fallow/untested-export",
        "No test reaches the export. A change to its behavior can break the production code that uses it, and no test fails.",
    ),
    (
        "fallow/code-duplication",
        "A bug fix in one copy does not reach the other copies. The copies drift apart, and each copy needs its own review and tests.",
    ),
    (
        "fallow/runtime-low-traffic",
        "Production almost never runs the function, but the team still maintains and tests it. Each change to it carries risk for a path that few requests need.",
    ),
    (
        "fallow/runtime-coverage-unavailable",
        "fallow has no runtime data for the function, so it cannot tell if production runs it. A decision to delete or keep the function has no runtime evidence.",
    ),
    (
        "fallow/runtime-coverage",
        "The runtime data gave no known verdict for the function. A decision to delete or keep the function has no runtime evidence until fallow can read the verdict.",
    ),
    (
        "fallow/coverage-intelligence-delete",
        "No code imports the code, production does not run it and no test reaches it. It costs reading and upkeep in each refactor, and nothing depends on it.",
    ),
    (
        "fallow/flag-retirement-candidate",
        "The flag no longer switches real behavior, but its branches stay in the code. Readers and tests keep both paths alive, and a dead path can come back on with a wrong flag value.",
    ),
    (
        "fallow/test-only-branch",
        "The tests run a path that production never runs. A passing test then does not prove that the production path works.",
    ),
];

/// The rationale and the example of one security catalogue category.
struct SecurityCategoryText {
    id: &'static str,
    rationale: &'static str,
    example: &'static str,
}

/// The rationale and an example line of each tainted-sink catalogue category.
/// The example shows a sink from the category's own matcher rows in
/// `crates/security/data/security_matchers.toml`.
const SECURITY_CATEGORY_TEXTS: &[SecurityCategoryText] = &[
    SecurityCategoryText {
        id: "security/dangerous-html",
        rationale: "If an attacker controls the value, the browser runs their script in the page (cross-site scripting). fallow does not prove that attacker input reaches the sink, so check the trace first.",
        example: "`element.innerHTML = comment.body;`",
    },
    SecurityCategoryText {
        id: "security/template-escape-bypass",
        rationale: "The template engine skips HTML escaping for the wrapped value. If an attacker controls it, their markup and script reach the page. Check the trace before you fix.",
        example: "`return new Handlebars.SafeString(user.bio);`",
    },
    SecurityCategoryText {
        id: "security/command-injection",
        rationale: "The shell parses the string, so an attacker who controls part of it can run other commands on the server. Check the trace before you fix.",
        example: "`child_process.exec('convert ' + req.query.file + ' out.png');`",
    },
    SecurityCategoryText {
        id: "security/code-injection",
        rationale: "The runtime executes the string as code. If an attacker controls it, they run code with the rights of the process. Check the trace before you fix.",
        example: "`eval(req.body.expression);`",
    },
    SecurityCategoryText {
        id: "security/dynamic-regex",
        rationale: "An attacker who controls the pattern can make the regular expression run for a long time and block the event loop. Check the trace before you fix.",
        example: "`new RegExp(req.query.filter);`",
    },
    SecurityCategoryText {
        id: "security/redos-regex",
        rationale: "The pattern can backtrack for a very long time on a crafted input string. An attacker can then block the event loop with one request.",
        example: "`/^(a+)+$/.test(req.query.name);`",
    },
    SecurityCategoryText {
        id: "security/resource-amplification",
        rationale: "A size that comes from the request controls how much memory the code allocates. One request with a large value can exhaust the memory of the process.",
        example: "`new Array(Number(req.query.count));`",
    },
    SecurityCategoryText {
        id: "security/dynamic-module-load",
        rationale: "If an attacker controls the module path, they choose which code the process loads and runs. Check the trace before you fix.",
        example: "`require('./plugins/' + req.params.name);`",
    },
    SecurityCategoryText {
        id: "security/sql-injection",
        rationale: "The database parses the string as SQL. An attacker who controls part of it can read or change data that the query must not touch. Check the trace before you fix.",
        example: "`db.query('SELECT * FROM users WHERE id = ' + req.params.id);`",
    },
    SecurityCategoryText {
        id: "security/ssrf",
        rationale: "The server sends a request to a URL that the input chooses. An attacker can reach internal services or cloud metadata endpoints through it. Check the trace before you fix.",
        example: "`fetch(req.query.url);`",
    },
    SecurityCategoryText {
        id: "security/secret-to-network",
        rationale: "A secret from the environment leaves the process in a network request. If the destination or the payload is not the intended one, the secret leaks.",
        example: "`fetch(endpoint, { body: process.env.API_KEY });`",
    },
    SecurityCategoryText {
        id: "security/path-traversal",
        rationale: "A path segment that contains `../` can point outside the intended directory. An attacker can then read or write other files on the server. Check the trace before you fix.",
        example: "`fs.readFile(path.join(uploadDir, req.params.name));`",
    },
    SecurityCategoryText {
        id: "security/header-injection",
        rationale: "A value with line breaks can add headers or split the response. An attacker can then set cookies or change the response that the client gets.",
        example: "`res.setHeader('Location', req.query.next);`",
    },
    SecurityCategoryText {
        id: "security/open-redirect",
        rationale: "The application sends users to a URL that the input chooses. An attacker can use a trusted link of your domain to send users to a phishing page.",
        example: "`res.redirect(req.query.returnTo);`",
    },
    SecurityCategoryText {
        id: "security/postmessage-wildcard-origin",
        rationale: "With `*` as the target origin, any page that holds the window reference receives the message, including a page of an attacker.",
        example: "`window.parent.postMessage({ token }, '*');`",
    },
    SecurityCategoryText {
        id: "security/tls-validation-disabled",
        rationale: "The client accepts any certificate, so an attacker on the network can read and change the traffic without a warning.",
        example: "`https.request(url, { rejectUnauthorized: false });`",
    },
    SecurityCategoryText {
        id: "security/cleartext-transport",
        rationale: "The connection is not encrypted. Anyone on the network path can read and change the data that it carries.",
        example: "`fetch('http://api.example.com/orders');`",
    },
    SecurityCategoryText {
        id: "security/electron-unsafe-webpreferences",
        rationale: "Web content in the window gets Node.js access or skips the same-origin policy. A script injected into the page can then read files and run programs on the user's machine.",
        example: "`new BrowserWindow({ webPreferences: { nodeIntegration: true } });`",
    },
    SecurityCategoryText {
        id: "security/world-writable-permission",
        rationale: "Mode `0o777` lets every user on the machine change the file. Another process can replace its content, for example a script that the application runs later.",
        example: "`fs.chmodSync(scriptPath, 0o777);`",
    },
    SecurityCategoryText {
        id: "security/insecure-temp-file",
        rationale: "Other users can predict a fixed path in a shared temporary folder. They can create the file first or link it to another file, so the application reads or overwrites the wrong data.",
        example: "`fs.writeFileSync('/tmp/report.json', data);`",
    },
    SecurityCategoryText {
        id: "security/mysql-multiple-statements",
        rationale: "The connection runs more than one statement per query. A small SQL injection anywhere in the application can then append statements that drop or change tables.",
        example: "`mysql.createConnection({ host, user, multipleStatements: true });`",
    },
    SecurityCategoryText {
        id: "security/permissive-cors",
        rationale: "The policy allows every origin to send credentialed requests. Any website that a signed-in user visits can then call the API as that user.",
        example: "`app.use(cors({ origin: '*', credentials: true }));`",
    },
    SecurityCategoryText {
        id: "security/insecure-cookie",
        rationale: "Without `httpOnly`, a script in the page can read the cookie. Without `secure`, the browser also sends it over plain HTTP, where others on the network can read it.",
        example: "`res.cookie('session', id, { httpOnly: false });`",
    },
    SecurityCategoryText {
        id: "security/mass-assignment",
        rationale: "All properties of the request object go into the target. An attacker can set fields that the form never shows, such as a role or an owner ID. Check the trace before you fix.",
        example: "`Object.assign(user, req.body);`",
    },
    SecurityCategoryText {
        id: "security/weak-crypto",
        rationale: "The algorithm comes from a value at run time, or it is MD5, SHA-1, DES or RC4. With a weak algorithm, an attacker can forge hashes or decrypt the data.",
        example: "`crypto.createHash(options.algorithm);`",
    },
    SecurityCategoryText {
        id: "security/insecure-randomness",
        rationale: "`Math.random()` and `pseudoRandomBytes` are predictable. A token, session ID or salt made with them can be guessed.",
        example: "`const resetToken = Math.random().toString(36);`",
    },
    SecurityCategoryText {
        id: "security/jwt-alg-none",
        rationale: "A token signed with `none` has no signature. Anyone can change its claims, and a verifier that accepts `none` treats the token as valid.",
        example: "`jwt.sign(payload, '', { algorithm: 'none' });`",
    },
    SecurityCategoryText {
        id: "security/jwt-verify-missing-algorithms",
        rationale: "Without an `algorithms` allowlist, the token header chooses the algorithm. An attacker can then pick an algorithm that lets them forge a valid token.",
        example: "`jwt.verify(token, publicKey, { audience: 'api' });`",
    },
    SecurityCategoryText {
        id: "security/deprecated-cipher",
        rationale: "`createCipher` derives the key without a salt and uses no random IV, so equal input gives equal output and the key is easy to brute-force. Node.js removed the function.",
        example: "`crypto.createCipher('aes-256-cbc', password);`",
    },
    SecurityCategoryText {
        id: "security/unsafe-buffer-alloc",
        rationale: "The buffer holds old memory of the process until the code overwrites it. If the code sends the buffer before it fills it, earlier data such as secrets can leak.",
        example: "`const buffer = Buffer.allocUnsafe(size);`",
    },
    SecurityCategoryText {
        id: "security/unsafe-deserialization",
        rationale: "The parser can create objects or run code from the input. An attacker who controls the input can then run code in the process. Check the trace before you fix.",
        example: "`yaml.load(req.body.config);`",
    },
    SecurityCategoryText {
        id: "security/angular-trusted-html",
        rationale: "The call turns off Angular's sanitizer for the value. If an attacker controls the value, their script runs in the page. Check the trace before you fix.",
        example: "`this.sanitizer.bypassSecurityTrustHtml(comment.body);`",
    },
    SecurityCategoryText {
        id: "security/nextjs-open-redirect",
        rationale: "The route sends users to a URL that the input chooses. An attacker can use a trusted link of your domain to send users to a phishing page.",
        example: "`redirect(request.nextUrl.searchParams.get('next'));`",
    },
    SecurityCategoryText {
        id: "security/dom-document-write",
        rationale: "The browser parses the written string as HTML. If an attacker controls it, their script runs in the page. Check the trace before you fix.",
        example: "`document.write(location.hash.slice(1));`",
    },
    SecurityCategoryText {
        id: "security/jquery-html",
        rationale: "jQuery parses the string as HTML and runs its scripts. If an attacker controls it, their script runs in the page. Check the trace before you fix.",
        example: "`$('#preview').html(comment.body);`",
    },
    SecurityCategoryText {
        id: "security/route-send-file",
        rationale: "The route sends a file from a path that the input chooses. With `../` in the path, an attacker can download other files from the server.",
        example: "`res.sendFile(req.params.file, { root: 'public' });`",
    },
    SecurityCategoryText {
        id: "security/webview-injection",
        rationale: "The WebView runs the string as a script in the loaded page. If an attacker controls it, their code runs with the page's access to the app bridge.",
        example: "`webViewRef.current.injectJavaScript(message.script);`",
    },
    SecurityCategoryText {
        id: "security/prototype-pollution",
        rationale: "A key such as `__proto__` in the input changes the prototype of all objects. Checks elsewhere in the process can then read values that an attacker set.",
        example: "`_.merge(settings, req.body);`",
    },
    SecurityCategoryText {
        id: "security/zip-slip",
        rationale: "An archive entry with `../` in its name can write outside the target folder. A crafted archive can then overwrite application files.",
        example: "`zip.extractAllTo(targetDir, true);`",
    },
    SecurityCategoryText {
        id: "security/nosql-injection",
        rationale: "The query filter comes from the input as a whole object. An attacker can send an operator such as `$ne` and match records that they must not see, for example to skip a password check.",
        example: "`users.findOne(req.body);`",
    },
    SecurityCategoryText {
        id: "security/ssti",
        rationale: "The template engine compiles a template that the input builds. An attacker can then run template code on the server.",
        example: "`ejs.render('<h1>' + req.query.title + '</h1>');`",
    },
    SecurityCategoryText {
        id: "security/xxe",
        rationale: "An XML parser that resolves external entities can read local files or send requests for the attacker. A crafted XML document is enough.",
        example: "`libxmljs.parseXml(req.body, { noent: true });`",
    },
    SecurityCategoryText {
        id: "security/secret-pii-log",
        rationale: "Logs go to more places and people than the data itself, and they stay for a long time. A secret or personal data in a log leaks to everyone who can read the logs.",
        example: "`console.log('login', req.body.password);`",
    },
    SecurityCategoryText {
        id: "security/xpath-injection",
        rationale: "The XPath engine parses the string as a query. An attacker who controls part of it can select other nodes, for example to skip a login check.",
        example: "`xpath.select(\"//user[name='\" + req.query.name + \"']\", doc);`",
    },
    SecurityCategoryText {
        id: "security/llm-call-injection",
        rationale: "Input from the request goes into the prompt. An attacker can write instructions that change what the model does, for example to leak the system prompt or call tools.",
        example: "`openai.chat.completions.create({ messages: [{ role: 'user', content: req.body.q }] });`",
    },
];

/// The "Why it matters" text of the rule with id `id`, when it has one of
/// its own.
#[must_use]
pub fn rule_rationale(id: &str) -> Option<&'static str> {
    RULE_RATIONALES
        .iter()
        .find(|(rule, _)| *rule == id)
        .map(|(_, rationale)| *rationale)
        .or_else(|| security_category_text(id).map(|text| text.rationale))
}

/// The example line of a tainted-sink catalogue category.
#[must_use]
pub fn security_category_example(id: &str) -> Option<&'static str> {
    security_category_text(id).map(|text| text.example)
}

fn security_category_text(id: &str) -> Option<&'static SecurityCategoryText> {
    SECURITY_CATEGORY_TEXTS.iter().find(|text| text.id == id)
}
