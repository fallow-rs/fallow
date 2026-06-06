const https = {
  request(_url: string, _options: { rejectUnauthorized: boolean }): void {},
};

const tls = {
  connect(_options: { rejectUnauthorized: boolean }): void {},
};

class Agent {
  constructor(_options: { rejectUnauthorized: boolean }) {}
}

https.request("https://example.com", {
  rejectUnauthorized: false,
});

tls.connect({
  rejectUnauthorized: false,
});

new Agent({
  rejectUnauthorized: false,
});
