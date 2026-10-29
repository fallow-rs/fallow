import { existsSync, realpathSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const API_ORIGIN = "https://api.github.com";
const AVATAR_ORIGIN = "https://avatars.githubusercontent.com";
const PAGE_SIZE = 100;
const REQUEST_TIMEOUT_MS = 30_000;
const AVATAR_SIZE = 64;
const AVATAR_BATCH_SIZE = 8;
const MAX_AVATAR_BYTES = 256_000;
const PNG_SIGNATURE = Buffer.from("89504e470d0a1a0a", "hex");
const COLUMNS = 12;
const CELL_SIZE = 56;
const IMAGE_SIZE = 48;
const LOGIN_PATTERN = /^[a-z\d](?:[a-z\d-]{0,37}[a-z\d])?$/iu;

const avatarUrl = (value) => {
  const url = new URL(value);
  if (url.origin !== AVATAR_ORIGIN || url.username || url.password) {
    throw new Error("Expected a GitHub avatar URL");
  }
  url.searchParams.set("s", String(AVATAR_SIZE));
  return url.href;
};

/** Collect human code contributors and issue/PR authors in alphabetical order. */
export const collectContributors = (contributors, issues) => {
  if (!Array.isArray(contributors) || !Array.isArray(issues)) {
    throw new Error("Expected GitHub contributor and issue arrays");
  }
  const people = new Map();
  for (const person of [...contributors, ...issues.map((issue) => issue.user)]) {
    if (
      person === null ||
      person?.type === "Bot" ||
      person?.login?.toLowerCase() === "ghost" ||
      person?.login?.endsWith("[bot]")
    )
      continue;
    if (
      person?.type !== "User" ||
      !LOGIN_PATTERN.test(person.login) ||
      !Number.isSafeInteger(person.id) ||
      person.id <= 0
    ) {
      throw new Error("Invalid GitHub contributor identity");
    }
    people.set(person.id, {
      login: person.login,
      id: person.id,
      avatar: avatarUrl(person.avatar_url),
    });
  }
  const names = new Set();
  return [...people.values()]
    .toSorted((a, b) => {
      const left = a.login.toLowerCase();
      const right = b.login.toLowerCase();
      return left < right ? -1 : left > right ? 1 : a.id - b.id;
    })
    .filter((person) => {
      const key = person.login.toLowerCase();
      if (names.has(key)) return false;
      names.add(key);
      return true;
    });
};

/** Generate the gallery and readable credit page from GitHub's public APIs. */
export const generateContributors = async ({
  repository,
  token,
  outputDirectory,
  fetchImpl = fetch,
}) => {
  if (!/^[a-z\d_.-]+\/[a-z\d_.-]+$/iu.test(repository)) {
    throw new Error("Expected an owner/repository name");
  }
  const headers = { Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2026-03-10" };
  if (token) headers.Authorization = `Bearer ${token}`;
  const pages = async (endpoint, parameters = {}) => {
    const rows = [];
    for (let page = 1; ; page++) {
      const url = new URL(`${API_ORIGIN}/repos/${repository}/${endpoint}`);
      url.search = new URLSearchParams({
        ...parameters,
        per_page: String(PAGE_SIZE),
        page: String(page),
      });
      const response = await fetchImpl(url.href, {
        headers,
        redirect: "error",
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      });
      if (!response.ok) throw new Error(`GitHub ${endpoint} request failed (${response.status})`);
      const data = await response.json();
      if (!Array.isArray(data)) throw new Error(`Expected a GitHub ${endpoint} array`);
      rows.push(...data);
      if (data.length < PAGE_SIZE) return rows;
    }
  };
  const contributors = await pages("contributors");
  const issues = await pages("issues", { state: "all", sort: "created", direction: "asc" });
  const people = collectContributors(contributors, issues);
  if (people.length === 0) throw new Error("Refusing to publish an empty contributor gallery");
  const avatars = [];
  for (let start = 0; start < people.length; start += AVATAR_BATCH_SIZE) {
    const batch = await Promise.all(
      people.slice(start, start + AVATAR_BATCH_SIZE).map(async (person) => {
        const response = await fetchImpl(person.avatar, {
          redirect: "error",
          signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
        });
        if (!response.ok) throw new Error(`Avatar request failed (${response.status})`);
        const type = response.headers.get("content-type")?.split(";")[0];
        if (!["image/png", "image/jpeg", "image/gif", "image/webp"].includes(type)) {
          throw new Error("Expected a raster GitHub avatar");
        }
        const bytes = Buffer.from(await response.arrayBuffer());
        if (bytes.length === 0 || bytes.length > MAX_AVATAR_BYTES)
          throw new Error("Invalid GitHub avatar size");
        // GitHub can serve resized PNG bytes while retaining an image/gif header.
        const mimeType = bytes.subarray(0, PNG_SIGNATURE.length).equals(PNG_SIGNATURE)
          ? "image/png"
          : type;
        return `data:${mimeType};base64,${bytes.toString("base64")}`;
      }),
    );
    avatars.push(...batch);
  }
  const width = Math.min(COLUMNS, people.length) * CELL_SIZE;
  const height = Math.ceil(people.length / COLUMNS) * CELL_SIZE;
  const images = people.map((person, index) => {
    const x = (index % COLUMNS) * CELL_SIZE;
    const y = Math.floor(index / COLUMNS) * CELL_SIZE;
    return `  <svg x="${x}" y="${y}" width="${IMAGE_SIZE}" height="${IMAGE_SIZE}" viewBox="0 0 48 48"><image width="48" height="48" href="${avatars[index]}" clip-path="url(#circle)"/><title>${person.login}</title></svg>`;
  });
  const svg = [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title names">`,
    '  <title id="title">People who contribute code, bug reports and ideas</title>',
    `  <desc id="names">${people.map((person) => person.login).join(", ")}</desc>`,
    '  <defs><clipPath id="circle"><circle cx="24" cy="24" r="24"/></clipPath></defs>',
    ...images,
    "</svg>",
    "",
  ].join("\n");
  const markdown = [
    "# Contributors",
    "",
    "Thank you to everyone who contributes code, bug reports and ideas.",
    "",
    "This list includes commit contributors and authors of open and closed issues and pull requests. It is updated daily. Bots and deleted accounts are excluded.",
    "",
    ...people.map((person) => `- [${person.login}](https://github.com/${person.login})`),
    "",
  ].join("\n");
  await mkdir(outputDirectory, { recursive: true });
  await writeFile(path.join(outputDirectory, "contributors.svg"), svg);
  await writeFile(path.join(outputDirectory, "CONTRIBUTORS.md"), markdown);
};

if (
  process.argv[1] &&
  existsSync(process.argv[1]) &&
  realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1])
) {
  generateContributors({
    repository: process.env.GITHUB_REPOSITORY,
    token: process.env.GH_TOKEN || process.env.GITHUB_TOKEN,
    outputDirectory: process.argv[2] || "contributors-output",
  }).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
