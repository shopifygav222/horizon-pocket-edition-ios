// The mod builds Worker against a GitHub that is a table in memory: `node --test`.
import assert from "node:assert/strict";
import test from "node:test";
import {
  BUILDER,
  buildableLists,
  builderPlans,
  buildKey,
  handle,
  readMods,
  WORKFLOW,
} from "./worker.js";

const REPO = "someone/horizon-pocket-edition-ios";
const TAG = "v1.1.0-shared-worlds-mods";
const DEVICE = "0123456789abcdef0123456789abcdef";
const NOW = Date.parse("2026-11-24T12:00:00Z");

/// A KV namespace: `get` and `put` over a Map, the lifetimes noted.
function store() {
  const map = new Map();
  return {
    map,
    ttl: new Map(),
    async get(key) {
      return map.get(key) ?? null;
    },
    async put(key, value, options) {
      map.set(key, value);
      this.ttl.set(key, options?.expirationTtl);
    },
  };
}

/// GitHub's API for one repository: its artifacts, its workflows' runs, and what was asked of it.
/// `buildable`: the repository's list of the mods a build can take (none: the file can't be read).
/// `builder`: the repository's word on the mod builder (packaging/ios/mod-builder.json; none: no
/// such file), `builderRuns` that workflow's runs (none: the workflow is not there) and
/// `builderDispatch` what starting it answers.
function github({
  artifacts = [],
  runs = [],
  jobs = {},
  dispatch = 204,
  buildable = null,
  rerun = 201,
  builder = null,
  builderRuns = null,
  builderDispatch = 204,
} = {}) {
  const asked = [];
  const lists = [];
  const fetcher = async (url, init = {}) => {
    const { pathname, searchParams, hostname } = new URL(url);
    if (hostname === "raw.githubusercontent.com") {
      // The public files: asked for without the token.
      assert.equal(init.headers?.authorization, undefined);
      lists.push(pathname);
      if (pathname === `/${REPO}/main/packaging/ios/mod-builder.json`) {
        if (builder === null) return new Response("not found", { status: 404 });
        if (builder === "down") return new Response("try later", { status: 503 });
        return typeof builder === "string" ? new Response(builder) : Response.json(builder);
      }
      assert.equal(pathname, `/${REPO}/main/packaging/ios/catalog-buildable.json`);
      if (buildable === null) return new Response("not found", { status: 404 });
      return typeof buildable === "string"
        ? new Response(buildable)
        : Response.json({ engine: "starling-0.9.0", mods: buildable });
    }
    asked.push({ method: init.method ?? "GET", pathname, body: init.body, init });
    assert.equal(init.headers.authorization, "Bearer token");
    const base = `/repos/${REPO}/actions`;
    if (pathname === `${base}/artifacts`) {
      const name = searchParams.get("name");
      return Response.json({ artifacts: artifacts.filter((a) => a.name === name) });
    }
    if (pathname === `${base}/workflows/${WORKFLOW}/runs`) {
      return Response.json({ workflow_runs: runs });
    }
    if (pathname === `${base}/workflows/${WORKFLOW}/dispatches`) {
      return new Response(null, { status: dispatch });
    }
    if (pathname === `${base}/workflows/${BUILDER}/runs`) {
      if (builderRuns === null) return new Response("not found", { status: 404 });
      return Response.json({ workflow_runs: builderRuns });
    }
    if (pathname === `${base}/workflows/${BUILDER}/dispatches`) {
      return new Response(null, { status: builderDispatch });
    }
    if (/\/runs\/\d+\/rerun-failed-jobs$/.test(pathname)) {
      assert.equal(init.method, "POST");
      return new Response(null, { status: rerun });
    }
    const job = /\/runs\/(\d+)\/jobs$/.exec(pathname);
    if (job) return Response.json({ jobs: jobs[job[1]] ?? [] });
    const zip = /\/artifacts\/(\d+)\/zip$/.exec(pathname);
    if (zip) {
      assert.equal(init.redirect, "manual");
      return new Response(null, {
        status: 302,
        headers: { location: `https://files.example/${zip[1]}?sig=short` },
      });
    }
    return new Response("not found", { status: 404 });
  };
  return { fetcher, asked, lists };
}

/// A fresh service each time: what it read of the repository's files is forgotten.
const env = (more = {}) => {
  builderPlans.clear();
  return { REPO, GITHUB_TOKEN: "token", LIMITS: store(), ...more };
};

const post = (body) =>
  new Request("https://builds.example/v1/build", { method: "POST", body: JSON.stringify(body) });

const get = (path) => new Request(`https://builds.example${path}`);

const asking = (mods = ["waymap@1.2.0", "appleskin@3.0.1"], more = {}) =>
  post({ tag: TAG, mods, device: DEVICE, ...more });

test("the same mods in any order are the same build", async () => {
  const a = readMods(["waymap@1.2.0", "appleskin@3.0.1"], 24).mods;
  const b = readMods(["appleskin@3.0.1", "waymap@1.2.0"], 24).mods;
  assert.deepEqual(a, ["appleskin@3.0.1", "waymap@1.2.0"]);
  assert.deepEqual(a, b);
  assert.equal(await buildKey(TAG, a), await buildKey(TAG, b));
  assert.match(await buildKey(TAG, a), /^[0-9a-f]{24}$/);
  // Another edition, another version of a mod: another build.
  assert.notEqual(await buildKey(TAG, a), await buildKey("v1.1.0", a));
  assert.notEqual(await buildKey(TAG, a), await buildKey(TAG, ["appleskin@3.0.2", "waymap@1.2.0"]));
});

test("what is not a catalog mod is refused before GitHub hears of it", async () => {
  const { fetcher, asked } = github();
  for (const mods of [
    [],
    "waymap@1.2.0",
    ["waymap"],
    ["Waymap@1.2.0"],
    ["waymap@1.2.0; rm -rf /"],
    ["../waymap@1"],
    ["waymap@1.2.0", "waymap@1.3.0"],
    Array.from({ length: 25 }, (_, i) => `mod${i}@1`),
  ]) {
    const response = await handle(asking(mods), env(), fetcher, NOW);
    assert.equal(response.status, 400, JSON.stringify(mods));
    assert.ok((await response.json()).error);
  }
  for (const body of [
    { tag: "main", mods: ["waymap@1"], device: DEVICE },
    { tag: "v1.1.0-other", mods: ["waymap@1"], device: DEVICE },
    { tag: TAG, mods: ["waymap@1"], device: "me" },
    { tag: TAG, mods: ["waymap@1"] },
  ]) {
    assert.equal((await handle(post(body), env(), fetcher, NOW)).status, 400);
  }
  const broken = new Request("https://builds.example/v1/build", { method: "POST", body: "{" });
  assert.equal((await handle(broken, env(), fetcher, NOW)).status, 400);
  assert.equal(asked.length, 0);
});

test("a new build is started once, at the release's own tag", async () => {
  const { fetcher, asked } = github();
  const e = env();
  const response = await handle(asking(), e, fetcher, NOW);
  assert.equal(response.status, 202);
  const { key, state } = await response.json();
  assert.equal(state, "queued");
  assert.equal(key, await buildKey(TAG, ["appleskin@3.0.1", "waymap@1.2.0"]));
  const started = asked.filter((a) => a.method === "POST");
  assert.equal(started.length, 1);
  assert.deepEqual(JSON.parse(started[0].body), {
    ref: TAG,
    inputs: { mods: "appleskin@3.0.1,waymap@1.2.0", key },
  });
  // Counted for the device and for the day, and remembered for the moment GitHub takes to list it.
  assert.equal(e.LIMITS.map.get(`d:2026-11-24:${DEVICE}`), "1");
  assert.equal(e.LIMITS.map.get("g:2026-11-24"), "1");
  assert.equal(e.LIMITS.map.get(`k:${key}`), "1");
  // Asked again a moment later (GitHub lists no run yet): the same build, not a second one.
  const again = await handle(asking(["waymap@1.2.0", "appleskin@3.0.1"]), e, fetcher, NOW + 5000);
  assert.deepEqual(await again.json(), { key, state: "queued" });
  assert.equal(asked.filter((a) => a.method === "POST").length, 1);
  assert.equal(e.LIMITS.map.get(`d:2026-11-24:${DEVICE}`), "1");
});

test("a build under way or finished is found, not started again", async () => {
  const key = await buildKey(TAG, ["appleskin@3.0.1", "waymap@1.2.0"]);
  const running = github({
    runs: [
      { id: 7, display_title: "mod-build 000000000000000000000000", status: "completed" },
      {
        id: 8,
        display_title: `mod-build ${key}`,
        status: "in_progress",
        run_started_at: "2026-11-24T11:50:00Z",
      },
    ],
  });
  const during = await handle(asking(), env(), running.fetcher, NOW);
  assert.deepEqual(await during.json(), {
    key,
    state: "building",
    since: "2026-11-24T11:50:00Z",
  });
  assert.equal(running.asked.filter((a) => a.method === "POST").length, 0);

  const done = github({
    artifacts: [
      { id: 41, name: `mod-build-${key}`, expired: true, created_at: "2026-11-01T00:00:00Z" },
      {
        id: 42,
        name: `mod-build-${key}`,
        expired: false,
        created_at: "2026-11-24T11:58:00Z",
        size_in_bytes: 123456789,
        expires_at: "2026-12-01T11:58:00Z",
      },
    ],
  });
  const ready = await handle(asking(), env(), done.fetcher, NOW);
  assert.deepEqual(await ready.json(), {
    key,
    state: "ready",
    size: 123456789,
    expires: "2026-12-01T11:58:00Z",
  });
  assert.equal(done.asked.filter((a) => a.method === "POST").length, 0);
  // Its file: GitHub's own link, handed on.
  const link = await handle(get(`/v1/build/${key}/ipa`), env(), done.fetcher, NOW);
  assert.equal(link.status, 302);
  assert.equal(link.headers.get("location"), "https://files.example/42?sig=short");
});

test("a failed build says where it failed and waits an hour", async () => {
  const key = await buildKey(TAG, ["appleskin@3.0.1", "waymap@1.2.0"]);
  const failed = (updated) =>
    github({
      runs: [
        {
          id: 9,
          display_title: `mod-build ${key}`,
          status: "completed",
          conclusion: "failure",
          updated_at: updated,
        },
      ],
      jobs: {
        9: [
          { conclusion: "success", steps: [{ name: "Check the request", conclusion: "success" }] },
          { conclusion: "failure", steps: [{ name: "Build the game", conclusion: "failure" }] },
        ],
      },
    });
  const fresh = failed("2026-11-24T11:40:00Z");
  const told = await handle(asking(), env(), fresh.fetcher, NOW);
  assert.deepEqual(await told.json(), {
    key,
    state: "failed",
    since: "2026-11-24T11:40:00Z",
    reason: "The build failed at: Build the game.",
  });
  assert.equal(fresh.asked.filter((a) => a.method === "POST").length, 0);
  // The app reads the same.
  const read = await handle(get(`/v1/build/${key}`), env(), fresh.fetcher, NOW);
  assert.equal((await read.json()).state, "failed");
  // An hour on, asking again tries again.
  const old = failed("2026-11-24T10:30:00Z");
  const retried = await handle(asking(), env(), old.fetcher, NOW);
  assert.equal(retried.status, 202);
  assert.equal(old.asked.filter((a) => a.method === "POST").length, 1);
});

test("a build GitHub had no machine for is started again, a few times", async () => {
  const key = await buildKey(TAG, ["appleskin@3.0.1", "waymap@1.2.0"]);
  // As GitHub lists it: the run failed, its build job cancelled with no step ever run.
  const unstarted = (more = {}, most = {}) =>
    github({
      runs: [
        {
          id: 9,
          display_title: `mod-build ${key}`,
          status: "completed",
          conclusion: "failure",
          run_attempt: 1,
          updated_at: "2026-11-24T11:40:00Z",
          ...more,
        },
      ],
      jobs: {
        9: [
          { conclusion: "success", steps: [{ name: "Check the request", conclusion: "success" }] },
          { conclusion: "cancelled", steps: [] },
          { conclusion: "skipped", steps: [] },
        ],
      },
      ...most,
    });
  const reruns = (g) => g.asked.filter((a) => a.pathname.endsWith("/rerun-failed-jobs")).length;
  // The app that waits reads the state: that starts the same run again, once, and counts for
  // nobody's day.
  const first = unstarted();
  const e = env();
  const read = await handle(get(`/v1/build/${key}`), e, first.fetcher, NOW);
  assert.deepEqual(await read.json(), { key, state: "queued" });
  assert.equal(reruns(first), 1);
  const again = await handle(get(`/v1/build/${key}`), e, first.fetcher, NOW);
  assert.deepEqual(await again.json(), { key, state: "queued" });
  const asked = await handle(asking(), e, first.fetcher, NOW);
  assert.equal((await asked.json()).state, "queued");
  assert.equal(reruns(first), 1);
  assert.deepEqual([...e.LIMITS.map.keys()], [`r:${key}`]);
  assert.equal(first.asked.filter((a) => a.pathname.endsWith("/dispatches")).length, 0);
  // The third try was the last: then it failed, and says what it was.
  const last = unstarted({ run_attempt: 3 });
  const told = await handle(get(`/v1/build/${key}`), env(), last.fetcher, NOW);
  assert.deepEqual(await told.json(), {
    key,
    state: "failed",
    since: "2026-11-24T11:40:00Z",
    reason: "GitHub had no machine free for the build.",
  });
  assert.equal(reruns(last), 0);
  // A run cancelled by hand stays cancelled, and so does one GitHub won't start again.
  const byHand = unstarted({ conclusion: "cancelled" });
  const left = await handle(get(`/v1/build/${key}`), env(), byHand.fetcher, NOW);
  assert.equal((await left.json()).state, "failed");
  assert.equal(reruns(byHand), 0);
  const refused = unstarted({}, { rerun: 403 });
  const kept = await handle(get(`/v1/build/${key}`), env(), refused.fetcher, NOW);
  assert.equal((await kept.json()).state, "failed");
  // A build that failed at a step is not started again by itself (the test above).
});

test("a device and a day have only so many builds", async () => {
  const { fetcher, asked } = github();
  const e = env({ PER_DEVICE_PER_DAY: "2", PER_DAY: "3" });
  const build = (n, device = DEVICE) =>
    handle(post({ tag: TAG, mods: [`mod${n}@1`], device }), e, fetcher, NOW);
  assert.equal((await build(1)).status, 202);
  assert.equal((await build(2)).status, 202);
  const third = await build(3);
  assert.equal(third.status, 429);
  assert.match((await third.json()).error, /2 builds today/);
  // Another device has its own, until the day's are gone.
  const other = "f".repeat(32);
  assert.equal((await build(4, other)).status, 202);
  assert.equal((await build(5, other)).status, 429);
  assert.equal(asked.filter((a) => a.method === "POST").length, 3);
  // Tomorrow there are more.
  const tomorrow = await handle(
    post({ tag: TAG, mods: ["mod6@1"], device: DEVICE }),
    e,
    fetcher,
    NOW + 24 * 60 * 60 * 1000,
  );
  assert.equal(tomorrow.status, 202);
});

test("a mod no build takes is refused before a build is started or counted", async () => {
  buildableLists.clear();
  const list = { waymap: "1.2.0", appleskin: "3.0.1" };
  const { fetcher, asked, lists } = github({ buildable: list });
  const e = env();
  const one = await handle(asking(["waymap@1.2.0", "voice_chat@1.0.3"]), e, fetcher, NOW);
  assert.equal(one.status, 400);
  assert.match((await one.json()).error, /^voice_chat can't be built into the app\./);
  const two = await handle(asking(["freecam_plus@2.0.1", "voice_chat@1.0.3"]), e, fetcher, NOW);
  assert.match((await two.json()).error, /^freecam_plus, voice_chat can't be built/);
  // GitHub was asked nothing, and the device's day is whole.
  assert.equal(asked.length, 0);
  assert.equal(e.LIMITS.map.size, 0);
  // A mod of the list is built, in a newer version too; the list was read once for all of this.
  const fine = await handle(asking(["waymap@1.3.0", "appleskin@3.0.1"]), e, fetcher, NOW);
  assert.equal(fine.status, 202);
  assert.equal(lists.filter((path) => path.endsWith("/catalog-buildable.json")).length, 1);
  // Read again when it is old; a list that can't be read then leaves the one known.
  const later = NOW + 11 * 60 * 1000;
  const gone = github();
  const still = await handle(asking(["voice_chat@1.0.3"]), env(), gone.fetcher, later);
  assert.equal(still.status, 400);
  assert.equal(gone.lists.length, 1);
  // No list at all (a fork without the file, GitHub's files down, not a list): nothing is refused.
  for (const buildable of [null, "<html>", "{}", '{"mods":{}}', '{"mods":[]}']) {
    buildableLists.clear();
    const none = github({ buildable });
    const open = await handle(asking(["voice_chat@1.0.3"]), env(), none.fetcher, NOW);
    assert.equal(open.status, 202, String(buildable));
  }
  buildableLists.clear();
});

const posts = (asked) => asked.filter((a) => a.method === "POST");
const MODS = ["appleskin@3.0.1", "waymap@1.2.0"];

test("without the builder's word every build is the release's own, as before", async () => {
  // No file, a file that does not read, one that names no branch, one whose releases are not
  // releases: nothing is the builder's.
  for (const builder of [
    null,
    "<html>",
    { tags: [TAG] },
    { ref: "main" },
    { ref: "main", tags: "all" },
    { ref: "main", tags: [TAG, "main"] },
    { ref: "feature/x", tags: [TAG] },
    { ref: "main", tags: [] },
    { ref: "main", tags: ["v1.1.0"] },
  ]) {
    const { fetcher, asked } = github({ builder, builderRuns: [] });
    const e = env();
    const response = await handle(asking(), e, fetcher, NOW);
    assert.equal(response.status, 202, JSON.stringify(builder));
    const { key } = await response.json();
    const started = posts(asked);
    assert.equal(started.length, 1);
    assert.ok(started[0].pathname.endsWith(`/workflows/${WORKFLOW}/dispatches`));
    assert.deepEqual(JSON.parse(started[0].body), {
      ref: TAG,
      inputs: { mods: MODS.join(","), key },
    });
    assert.equal(e.LIMITS.map.get(`w:${key}`), "o");
  }
});

test("a release the builder builds is started with it, on its branch, as a player's build", async () => {
  const { fetcher, asked, lists } = github({
    builder: { ref: "main", tags: ["v1.1.1", TAG] },
    builderRuns: [],
  });
  const e = env();
  const response = await handle(asking(), e, fetcher, NOW);
  assert.equal(response.status, 202);
  const { key, state } = await response.json();
  assert.equal(state, "queued");
  // The same key as ever: the build is found by it whichever workflow made it.
  assert.equal(key, await buildKey(TAG, MODS));
  const started = posts(asked);
  assert.equal(started.length, 1);
  assert.ok(started[0].pathname.endsWith(`/workflows/${BUILDER}/dispatches`));
  assert.deepEqual(JSON.parse(started[0].body), {
    ref: "main",
    inputs: { tag: TAG, mods: MODS.join(","), key, serve: "true" },
  });
  assert.equal(e.LIMITS.map.get(`w:${key}`), "b");
  assert.equal(e.LIMITS.ttl.get(`w:${key}`), 24 * 60 * 60);
  assert.equal(e.LIMITS.map.get(`d:2026-11-24:${DEVICE}`), "1");
  // The repository's word is read once for a while, not for every request.
  const again = await handle(asking(), e, fetcher, NOW + 5000);
  assert.deepEqual(await again.json(), { key, state: "queued" });
  assert.equal(posts(asked).length, 1);
  assert.equal(lists.filter((path) => path.endsWith("/mod-builder.json")).length, 1);
  // Another release, which the builder does not build: its own workflow, at its tag.
  const other = await handle(
    post({ tag: "v1.0.0", mods: MODS, device: DEVICE }),
    e,
    fetcher,
    NOW + 6000,
  );
  assert.equal(other.status, 202);
  const last = posts(asked).at(-1);
  assert.ok(last.pathname.endsWith(`/workflows/${WORKFLOW}/dispatches`));
  assert.equal(JSON.parse(last.body).ref, "v1.0.0");
});

test("the builder's word is read again after a while, and kept when it can't be read", async () => {
  const on = github({ builder: { ref: "main", tags: [TAG] }, builderRuns: [] });
  const e = env();
  await handle(asking(), e, on.fetcher, NOW);
  assert.ok(posts(on.asked)[0].pathname.endsWith(`/workflows/${BUILDER}/dispatches`));
  // Minutes later the file can't be fetched: the builder stays on.
  const down = github({ builder: "down", builderRuns: [] });
  await handle(asking(["waymap@1.2.0"]), e, down.fetcher, NOW + 5 * 60 * 1000);
  assert.ok(posts(down.asked)[0].pathname.endsWith(`/workflows/${BUILDER}/dispatches`));
  // Later the file is gone (the builder was turned off): the release builds itself.
  const off = github({ builder: null, builderRuns: [] });
  await handle(asking(["appleskin@3.0.1"]), e, off.fetcher, NOW + 10 * 60 * 1000);
  assert.ok(posts(off.asked)[0].pathname.endsWith(`/workflows/${WORKFLOW}/dispatches`));
});

test("when the builder can't be started the release builds itself", async () => {
  for (const builderDispatch of [404, 422, 500]) {
    const { fetcher, asked } = github({
      builder: { ref: "main", tags: [TAG] },
      builderRuns: [],
      builderDispatch,
    });
    const e = env();
    const response = await handle(asking(), e, fetcher, NOW);
    assert.equal(response.status, 202, String(builderDispatch));
    const { key } = await response.json();
    const started = posts(asked);
    assert.equal(started.length, 2);
    assert.ok(started[0].pathname.endsWith(`/workflows/${BUILDER}/dispatches`));
    assert.ok(started[1].pathname.endsWith(`/workflows/${WORKFLOW}/dispatches`));
    assert.deepEqual(JSON.parse(started[1].body), {
      ref: TAG,
      inputs: { mods: MODS.join(","), key },
    });
    // Remembered as the release's own build, and counted once.
    assert.equal(e.LIMITS.map.get(`w:${key}`), "o");
    assert.equal(e.LIMITS.map.get(`d:2026-11-24:${DEVICE}`), "1");
  }
});

test("a build the builder is making is found by its key, and only where it was started", async () => {
  const key = await buildKey(TAG, MODS);
  const running = {
    id: 9,
    display_title: `mod-build ${key}`,
    status: "in_progress",
    created_at: "2026-11-24T11:50:00Z",
    run_started_at: "2026-11-24T11:51:00Z",
  };
  // Remembered as the builder's: its workflow alone is asked.
  const mine = github({ builder: { ref: "main", tags: [TAG] }, builderRuns: [running] });
  const e = env();
  e.LIMITS.map.set(`w:${key}`, "b");
  const during = await handle(get(`/v1/build/${key}`), e, mine.fetcher, NOW);
  assert.deepEqual(await during.json(), { key, state: "building", since: "2026-11-24T11:51:00Z" });
  assert.equal(mine.asked.filter((a) => a.pathname.endsWith(`/${WORKFLOW}/runs`)).length, 0);
  assert.equal(mine.lists.length, 0);
  // Remembered as the release's own: the builder's is not asked, nor the repository's word.
  const own = github({ builder: { ref: "main", tags: [TAG] }, builderRuns: [running] });
  const o = env();
  o.LIMITS.map.set(`w:${key}`, "o");
  const none = await handle(get(`/v1/build/${key}`), o, own.fetcher, NOW);
  assert.deepEqual(await none.json(), { key, state: "none" });
  assert.equal(own.asked.filter((a) => a.pathname.endsWith(`/${BUILDER}/runs`)).length, 0);
  assert.equal(own.lists.length, 0);
  // Nobody remembers (a day went by): both are asked, and the newer run is the build's.
  const older = {
    id: 5,
    display_title: `mod-build ${key}`,
    status: "completed",
    conclusion: "failure",
    created_at: "2026-11-24T09:00:00Z",
    updated_at: "2026-11-24T09:30:00Z",
  };
  const both = github({
    builder: { ref: "main", tags: [TAG] },
    runs: [older],
    builderRuns: [running],
  });
  const found = await handle(get(`/v1/build/${key}`), env(), both.fetcher, NOW);
  assert.equal((await found.json()).state, "building");
  // And a build asked for while the builder makes it is not started again.
  const again = await handle(asking(), env(), both.fetcher, NOW);
  assert.equal((await again.json()).state, "building");
  assert.equal(posts(both.asked).length, 0);
  // Without the builder's word, and with no such workflow, only the release's own run counts.
  const before = github({ runs: [older] });
  const failed = await handle(get(`/v1/build/${key}`), env(), before.fetcher, NOW);
  assert.equal((await failed.json()).state, "failed");
  assert.equal(before.asked.filter((a) => a.pathname.endsWith(`/${BUILDER}/runs`)).length, 0);
  const absent = github({ builder: { ref: "main", tags: [TAG] }, runs: [older] });
  const still = await handle(get(`/v1/build/${key}`), env(), absent.fetcher, NOW);
  assert.equal((await still.json()).state, "failed");
});

test("a build GitHub had no machine for is started again in the builder too", async () => {
  const key = await buildKey(TAG, MODS);
  const { fetcher, asked } = github({
    builder: { ref: "main", tags: [TAG] },
    builderRuns: [
      {
        id: 31,
        display_title: `mod-build ${key}`,
        status: "completed",
        conclusion: "failure",
        run_attempt: 1,
        created_at: "2026-11-24T11:00:00Z",
        updated_at: "2026-11-24T11:45:00Z",
      },
    ],
    jobs: { 31: [{ name: "Build the app", conclusion: "cancelled", steps: [] }] },
  });
  const e = env();
  e.LIMITS.map.set(`w:${key}`, "b");
  const response = await handle(get(`/v1/build/${key}`), e, fetcher, NOW);
  assert.deepEqual(await response.json(), { key, state: "queued" });
  const again = posts(asked);
  assert.equal(again.length, 1);
  assert.ok(again[0].pathname.endsWith("/runs/31/rerun-failed-jobs"));
});

test("a file a pull request's run kept under a build's name is nobody's build", async () => {
  const key = await buildKey(TAG, MODS);
  const file = (id, run) => ({
    id,
    name: `mod-build-${key}`,
    expired: false,
    created_at: `2026-11-24T11:5${id}:00Z`,
    size_in_bytes: 1000 + id,
    expires_at: "2026-12-01T11:58:00Z",
    workflow_run: run,
  });
  // The newest is a fork's (its code came from another repository): the repository's own is the build.
  const mixed = github({
    artifacts: [
      file(1, { id: 70, repository_id: 5, head_repository_id: 5 }),
      file(2, { id: 71, repository_id: 5, head_repository_id: 99 }),
    ],
  });
  const ready = await handle(get(`/v1/build/${key}`), env(), mixed.fetcher, NOW);
  assert.equal((await ready.json()).size, 1001);
  const link = await handle(get(`/v1/build/${key}/ipa`), env(), mixed.fetcher, NOW);
  assert.equal(link.headers.get("location"), "https://files.example/1?sig=short");
  // Only a fork's: there is no build, and none is handed out.
  const forked = github({ artifacts: [file(2, { id: 71, repository_id: 5, head_repository_id: 99 })] });
  const none = await handle(get(`/v1/build/${key}`), env(), forked.fetcher, NOW);
  assert.deepEqual(await none.json(), { key, state: "none" });
  assert.equal((await handle(get(`/v1/build/${key}/ipa`), env(), forked.fetcher, NOW)).status, 404);
});

test("a build nobody made, a wrong address and a service not set up say so", async () => {
  const { fetcher } = github();
  const none = await handle(get(`/v1/build/${"0".repeat(24)}`), env(), fetcher, NOW);
  assert.deepEqual(await none.json(), { key: "0".repeat(24), state: "none" });
  assert.equal((await handle(get(`/v1/build/${"0".repeat(24)}/ipa`), env(), fetcher, NOW)).status, 404);
  assert.equal((await handle(get("/v1/build/not-a-key"), env(), fetcher, NOW)).status, 400);
  assert.equal((await handle(get("/v1/other"), env(), fetcher, NOW)).status, 404);
  assert.equal((await handle(get("/v1/build"), env(), fetcher, NOW)).status, 405);
  assert.equal((await handle(get("/"), env(), fetcher, NOW)).status, 200);
  // No token, no repository, no counters: nothing is started.
  assert.equal((await handle(asking(), { REPO }, fetcher, NOW)).status, 503);
  assert.equal((await handle(asking(), { GITHUB_TOKEN: "token" }, fetcher, NOW)).status, 503);
  const uncounted = await handle(asking(), { REPO, GITHUB_TOKEN: "token" }, fetcher, NOW);
  assert.equal(uncounted.status, 503);
  // A tag the repository has no workflow at.
  const missing = github({ dispatch: 422 });
  const refused = await handle(asking(), env(), missing.fetcher, NOW);
  assert.equal(refused.status, 400);
  // GitHub down: said, not hidden.
  const down = async () => new Response("no", { status: 500 });
  assert.equal((await handle(asking(), env(), down, NOW)).status, 502);
});
