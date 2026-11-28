// Horizon PE's mod builds: what the app asks when a player wants the app with catalog mods in it.
//
// An iPhone runs no code its app did not ship with, so a mod is compiled into the app itself. That
// build is this repository's `mod-build` workflow on GitHub; starting one takes a GitHub account
// with rights here, which a player doesn't have and shouldn't need. This Worker holds one token
// that may start that workflow and nothing else, and does three things for the app:
//
//   POST /v1/build            { tag, mods: ["id@version", ...], device }   start (or find) a build
//   GET  /v1/build/<key>      where that build stands
//   GET  /v1/build/<key>/ipa  the finished build's file (a redirect to GitHub's own short-lived link)
//
// A build is named by its key: the release tag and the sorted mods, hashed. The same request from
// anyone is the same build, made once. The Worker keeps no account, no address and no list of what
// anyone asked for: only counters (how many builds a device and everyone started today), which
// expire. `device` is a random number the app makes at its first start.
//
// Bindings (wrangler.toml, README.md): GITHUB_TOKEN (secret), REPO, LIMITS (KV), and optionally
// PER_DEVICE_PER_DAY, PER_DAY, MAX_MODS.
//
// Two workflows build: `mod-build.yml`, at the release's own tag, compiles the whole game each
// time; `mod-builder.yml`, on a branch, compiles a release's game once and after that only the
// mods (README.md "The mod builder"). Which releases the second one builds is the repository's
// own word, packaging/ios/mod-builder.json on its main branch: without that file, or for a
// release it does not name, everything is the first workflow's, as it always was.

export const WORKFLOW = "mod-build.yml";
export const BUILDER = "mod-builder.yml";
// This repository's release tags, the three editions': the workflow at that tag builds itself.
const TAG = /^v\d{1,3}\.\d{1,3}\.\d{1,3}(-shared-worlds(-mods)?)?$/;
// A catalog mod and its version, as the registry's index names them.
const MOD = /^([a-z0-9][a-z0-9_]{0,63})@([0-9A-Za-z][0-9A-Za-z.+_-]{0,31})$/;
const DEVICE = /^[0-9a-f]{32}$/;
const KEY = /^[0-9a-f]{24}$/;
const REPO = /^[A-Za-z0-9_.-]{1,100}\/[A-Za-z0-9_.-]{1,100}$/;
// A build that failed is not started again at once: the same source fails the same way.
const FAILED_HOLD_MS = 60 * 60 * 1000;
// GitHub lists a run a few seconds after it is asked for: meanwhile the key is remembered as
// started, so two requests a moment apart start one build.
const STARTED_TTL = 180;
// The catalog's mods a build can take (the repository's packaging/ios/catalog-buildable.json, on
// its main branch): read at most this often.
const BUILDABLE_FRESH_MS = 10 * 60 * 1000;
// A build GitHub found no machine for is started again by itself, this many times in all.
const MOST_ATTEMPTS = 3;
/// The lists read, by repository: { at, ids }.
export const buildableLists = new Map();
// Which releases the mod builder builds (packaging/ios/mod-builder.json): read at most this often,
// so turning it on or off is felt within minutes.
const BUILDER_FRESH_MS = 2 * 60 * 1000;
// A branch the builder runs on: a plain name.
const BRANCH = /^[A-Za-z0-9._-]{1,100}$/;
// Which workflow a build was started with is remembered this long (seconds): its run is then
// looked for in that workflow alone.
const STARTED_BY_TTL = 24 * 60 * 60;
/// The builder's word read, by repository: { at, plan }, `plan` null when it builds nothing.
export const builderPlans = new Map();

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" },
  });

const refuse = (status, error, more = {}) => json({ error, ...more }, status);

const number = (value, fallback) => {
  const n = Number.parseInt(value ?? "", 10);
  return Number.isFinite(n) && n >= 0 ? n : fallback;
};

/// The mods of a request, checked, one version an id, sorted: the same set is the same list.
export function readMods(mods, most) {
  if (!Array.isArray(mods) || mods.length === 0) return { error: "No mods were chosen." };
  if (mods.length > most) return { error: `A build takes at most ${most} mods.` };
  const seen = new Map();
  for (const mod of mods) {
    const match = typeof mod === "string" ? MOD.exec(mod) : null;
    if (!match) return { error: "A mod is not named as the catalog names it (id@version)." };
    if (seen.has(match[1])) return { error: `${match[1]} was chosen twice.` };
    seen.set(match[1], mod);
  }
  return { mods: [...seen.keys()].sort().map((id) => seen.get(id)) };
}

/// The build's name: the tag and the mods, hashed. The workflow computes it again and refuses a
/// request whose key is not this (tools/mod_build_key.py).
export async function buildKey(tag, mods) {
  const bytes = new TextEncoder().encode(`${tag}\n${mods.join(",")}`);
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...hash.slice(0, 12)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/// The ids of the catalog's mods a build can take, or null when the list can't be read: then
/// nothing is refused by it, and a mod no build takes fails in the build itself, as before.
async function buildable(repo, fetcher, now) {
  const known = buildableLists.get(repo);
  if (known && now - known.at < BUILDABLE_FRESH_MS) return known.ids;
  try {
    const response = await fetcher(
      `https://raw.githubusercontent.com/${repo}/main/packaging/ios/catalog-buildable.json`,
      { headers: { "user-agent": "horizon-pe-mod-builds" } },
    );
    if (!response.ok) return known?.ids ?? null;
    const mods = (await response.json())?.mods;
    if (!mods || typeof mods !== "object" || Array.isArray(mods)) return known?.ids ?? null;
    const ids = new Set(Object.keys(mods));
    if (ids.size === 0) return known?.ids ?? null;
    buildableLists.set(repo, { at: now, ids });
    return ids;
  } catch {
    return known?.ids ?? null;
  }
}

/// What the repository says the mod builder builds: { ref, tags }, the branch it runs on and the
/// releases it builds. Null when it says nothing (no file, or one that does not read): then no
/// build is the builder's. A file that can't be fetched this once leaves what was known.
async function builder(repo, fetcher, now) {
  const known = builderPlans.get(repo);
  if (known && now - known.at < BUILDER_FRESH_MS) return known.plan;
  const keep = (plan) => {
    builderPlans.set(repo, { at: now, plan });
    return plan;
  };
  try {
    const response = await fetcher(
      `https://raw.githubusercontent.com/${repo}/main/packaging/ios/mod-builder.json`,
      { headers: { "user-agent": "horizon-pe-mod-builds" } },
    );
    if (response.status === 404) return keep(null);
    if (!response.ok) return known?.plan ?? null;
    const said = await response.json();
    const tags = Array.isArray(said?.tags) ? said.tags : null;
    if (
      typeof said?.ref !== "string" ||
      !BRANCH.test(said.ref) ||
      !tags ||
      !tags.every((tag) => typeof tag === "string" && TAG.test(tag))
    ) {
      return keep(null);
    }
    return keep({ ref: said.ref, tags: new Set(tags) });
  } catch {
    return known?.plan ?? null;
  }
}

/// Why these mods are not built, when one of them is a mod no build takes; else null.
export function unbuildable(mods, ids) {
  if (!ids) return null;
  const not = mods.map((mod) => mod.split("@")[0]).filter((id) => !ids.has(id));
  if (not.length === 0) return null;
  return not.length === 1
    ? `${not[0]} can't be built into the app. Turn it off and build again.`
    : `${not.join(", ")} can't be built into the app. Turn them off and build again.`;
}

function settings(env) {
  const repo = env.REPO ?? "";
  if (!REPO.test(repo) || !env.GITHUB_TOKEN) return null;
  return {
    repo,
    token: env.GITHUB_TOKEN,
    perDevice: number(env.PER_DEVICE_PER_DAY, 6),
    perDay: number(env.PER_DAY, 300),
    maxMods: number(env.MAX_MODS, 24),
  };
}

/// GitHub's API for this repository, with the Worker's token.
function github(s, fetcher) {
  return (path, init = {}) =>
    fetcher(`https://api.github.com/repos/${s.repo}${path}`, {
      ...init,
      headers: {
        authorization: `Bearer ${s.token}`,
        accept: "application/vnd.github+json",
        "x-github-api-version": "2022-11-28",
        "user-agent": "horizon-pe-mod-builds",
        ...(init.headers ?? {}),
      },
    });
}

/// Whether a run of this repository's own code made the artifact. A pull request from a fork
/// runs the fork's workflows, which may keep a file under any name: such a file is nobody's
/// build. (GitHub says where a run's code came from; a file it says nothing of is taken as before.)
const ours = (a) =>
  !a.workflow_run ||
  a.workflow_run.head_repository_id == null ||
  a.workflow_run.head_repository_id === a.workflow_run.repository_id;

/// The finished build's artifact, if GitHub still keeps it.
async function artifact(api, key) {
  const response = await api(`/actions/artifacts?name=mod-build-${key}&per_page=10`);
  if (!response.ok) throw new Error(`GitHub answered ${response.status} for the build's file`);
  const { artifacts = [] } = await response.json();
  return (
    artifacts
      .filter((a) => !a.expired && a.name === `mod-build-${key}` && ours(a))
      .sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at))[0] ?? null
  );
}

/// The newest run of `workflow` for this key, among the workflow's last hundred.
async function runOf(api, key, workflow) {
  const response = await api(
    `/actions/workflows/${workflow}/runs?event=workflow_dispatch&per_page=100`,
  );
  // The builder's workflow may not be there (a repository that never had it): then it has no runs.
  if (response.status === 404 && workflow === BUILDER) return null;
  if (!response.ok) throw new Error(`GitHub answered ${response.status} for the builds`);
  const { workflow_runs: runs = [] } = await response.json();
  return runs.find((r) => r.display_title === `mod-build ${key}`) ?? null;
}

/// The newest run for this key. The workflow it was started with is remembered for a day
/// (`w:<key>`) and looked in alone; a build nobody remembers is looked for in the release's own
/// workflow, and in the builder's too when the repository has the builder's word (`plan`, asked
/// for only then).
async function run(api, kv, key, plan) {
  const by = kv ? await kv.get(`w:${key}`) : null;
  if (by === "b") return runOf(api, key, BUILDER);
  const own = await runOf(api, key, WORKFLOW);
  if (by === "o" || !(await plan())) return own;
  const built = await runOf(api, key, BUILDER);
  if (!own || !built) return own ?? built;
  return Date.parse(built.created_at) > Date.parse(own.created_at) ? built : own;
}

/// Why a run failed, in a line: the step that stopped it. `unstarted`: no step failed and a job
/// was given up before any machine took it (GitHub's Macs were all busy: it cancels such a job
/// after a while), which is no fault of the mods.
async function failure(api, id) {
  const plain = { reason: "The build failed.", unstarted: false };
  try {
    const response = await api(`/actions/runs/${id}/jobs?per_page=30`);
    if (!response.ok) return plain;
    const { jobs = [] } = await response.json();
    for (const job of jobs) {
      const step = (job.steps ?? []).find((s) => s.conclusion === "failure");
      if (step) return { reason: `The build failed at: ${step.name}.`, unstarted: false };
    }
    const never = (j) => j.conclusion === "cancelled" && (j.steps ?? []).length === 0;
    if (jobs.some(never)) {
      return { reason: "GitHub had no machine free for the build.", unstarted: true };
    }
    const job = jobs.find((j) => j.conclusion && j.conclusion !== "success");
    return job
      ? { reason: `The build ${job.conclusion.replace("_", " ")}.`, unstarted: false }
      : plain;
  } catch {
    return plain;
  }
}

/// Where the build with this key stands. `plan`: the mod builder's word, asked for when needed.
export async function state(api, kv, key, plan = async () => null) {
  const file = await artifact(api, key);
  if (file) {
    return { key, state: "ready", size: file.size_in_bytes, expires: file.expires_at };
  }
  const latest = await run(api, kv, key, plan);
  if (latest && latest.status !== "completed") {
    return {
      key,
      state: latest.status === "in_progress" ? "building" : "queued",
      since: latest.run_started_at ?? latest.created_at,
    };
  }
  if (latest && latest.conclusion !== "success") {
    const why = await failure(api, latest.id);
    // A build no machine took is started again, the same run: whoever asks next (the app that
    // waits for it reads its state every little while) sets it going, a few times at most. A
    // run someone cancelled by hand is left alone.
    if (
      kv &&
      why.unstarted &&
      latest.conclusion === "failure" &&
      (latest.run_attempt ?? 1) < MOST_ATTEMPTS
    ) {
      // GitHub lists the run as queued a moment after it is asked to: meanwhile it is remembered.
      if (await kv.get(`r:${key}`)) return { key, state: "queued" };
      const again = await api(`/actions/runs/${latest.id}/rerun-failed-jobs`, { method: "POST" });
      if (again.ok) {
        await kv.put(`r:${key}`, "1", { expirationTtl: STARTED_TTL });
        return { key, state: "queued" };
      }
    }
    return { key, state: "failed", since: latest.updated_at, reason: why.reason };
  }
  // Asked for a moment ago, not listed yet.
  if (kv && (await kv.get(`k:${key}`))) return { key, state: "queued" };
  // Never built, or built and its file has expired.
  return { key, state: "none" };
}

const day = (now) => new Date(now).toISOString().slice(0, 10);

/// Counts one more build for the device and for everyone today, unless either is at its limit.
async function allow(kv, s, device, now) {
  const [mine, all] = [`d:${day(now)}:${device}`, `g:${day(now)}`];
  const [used, total] = await Promise.all([kv.get(mine), kv.get(all)]);
  if (number(used, 0) >= s.perDevice) {
    return `This device has started ${s.perDevice} builds today. Tomorrow there are more.`;
  }
  if (number(total, 0) >= s.perDay) {
    return "Too many builds were asked for today. Tomorrow there are more.";
  }
  const kept = { expirationTtl: 2 * 24 * 60 * 60 };
  await Promise.all([
    kv.put(mine, String(number(used, 0) + 1), kept),
    kv.put(all, String(number(total, 0) + 1), kept),
  ]);
  return null;
}

async function start(request, env, s, api, fetcher, now) {
  let body;
  try {
    body = await request.json();
  } catch {
    return refuse(400, "The request is not JSON.");
  }
  const { tag, device } = body ?? {};
  if (typeof tag !== "string" || !TAG.test(tag)) {
    return refuse(400, "This app's version has no builds with mods.");
  }
  if (typeof device !== "string" || !DEVICE.test(device)) {
    return refuse(400, "The request names no device.");
  }
  const read = readMods(body.mods, s.maxMods);
  if (read.error) return refuse(400, read.error);
  // A mod that changes the game's own source is in the catalog and in no build: said now, not
  // by a build that fails some minutes in and counts against the device's day.
  const not = unbuildable(read.mods, await buildable(s.repo, fetcher, now));
  if (not) return refuse(400, not);
  const key = await buildKey(tag, read.mods);
  const kv = env.LIMITS;
  const plan = () => builder(s.repo, fetcher, now);
  const known = await state(api, kv, key, plan);
  if (known.state === "ready" || known.state === "queued" || known.state === "building") {
    return json(known);
  }
  if (known.state === "failed" && now - Date.parse(known.since) < FAILED_HOLD_MS) {
    return json(known);
  }
  // Without its counters the Worker would start builds for anyone without end.
  if (!kv) return refuse(503, "The build service is not set up (no LIMITS store).");
  const full = await allow(kv, s, device, now);
  if (full) return refuse(429, full);
  const mods = read.mods.join(",");
  const post = (workflow, body) =>
    api(`/actions/workflows/${workflow}/dispatches`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
  // A release the mod builder builds: its workflow, on its branch, told the release and that the
  // build is a player's (`serve`: the file is then named as this Worker finds it).
  const fast = await plan();
  let by = "o";
  let asked = null;
  if (fast?.tags.has(tag)) {
    asked = await post(BUILDER, { ref: fast.ref, inputs: { tag, mods, key, serve: "true" } });
    if (asked.ok) by = "b";
    // The builder can't be started: the release builds itself, as before.
    else asked = null;
  }
  // The workflow as the release itself has it, at the release's tag.
  if (!asked) asked = await post(WORKFLOW, { ref: tag, inputs: { mods, key } });
  if (asked.status === 404 || asked.status === 422) {
    return refuse(400, "This app's version has no builds with mods.");
  }
  if (!asked.ok) return refuse(502, `GitHub did not start the build (${asked.status}).`);
  await Promise.all([
    kv.put(`k:${key}`, "1", { expirationTtl: STARTED_TTL }),
    kv.put(`w:${key}`, by, { expirationTtl: STARTED_BY_TTL }),
  ]);
  return json({ key, state: "queued" }, 202);
}

async function file(api, key) {
  const found = await artifact(api, key);
  if (!found) return refuse(404, "This build's file is gone: build it again.");
  // GitHub answers with a link of its own that needs no token and lasts a minute.
  const link = await api(`/actions/artifacts/${found.id}/zip`, { redirect: "manual" });
  const location = link.headers.get("location");
  if (link.status !== 302 || !location) {
    return refuse(502, `GitHub did not hand the build's file over (${link.status}).`);
  }
  return new Response(null, { status: 302, headers: { location, "cache-control": "no-store" } });
}

export async function handle(request, env, fetcher = fetch, now = Date.now()) {
  const url = new URL(request.url);
  const parts = url.pathname.split("/").filter(Boolean);
  if (parts.length === 0 && request.method === "GET") {
    return json({ service: "horizon-pe-mod-builds", api: 1 });
  }
  if (parts[0] !== "v1" || parts[1] !== "build" || parts.length > 4) {
    return refuse(404, "No such address.");
  }
  const s = settings(env);
  if (!s) return refuse(503, "The build service is not set up (REPO, GITHUB_TOKEN).");
  const api = github(s, fetcher);
  try {
    if (parts.length === 2) {
      if (request.method !== "POST") return refuse(405, "A build is asked for with POST.");
      return await start(request, env, s, api, fetcher, now);
    }
    if (request.method !== "GET") return refuse(405, "A build is read with GET.");
    if (!KEY.test(parts[2])) return refuse(400, "That is not a build's key.");
    if (parts.length === 3) {
      const plan = () => builder(s.repo, fetcher, now);
      return json(await state(api, env.LIMITS, parts[2], plan));
    }
    if (parts[3] === "ipa") return await file(api, parts[2]);
    return refuse(404, "No such address.");
  } catch (error) {
    return refuse(502, String(error?.message ?? error));
  }
}

export default {
  fetch: (request, env) => handle(request, env),
};
