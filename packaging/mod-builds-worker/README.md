# Horizon PE's mod builds: setting the service up

This is what lets a player tap **Build** in the game and get the app with their mods, with no
GitHub account of their own. You set it up once. It takes three steps and about ten minutes, and
all three can be done in a phone's browser.

It is free: one small Cloudflare Worker on Cloudflare's free plan. It keeps no accounts and no
record of who asked for what.

## Step 1. Make a GitHub token

The service starts builds with a token that can do that and nothing else.

1. Open <https://github.com/settings/personal-access-tokens/new>.
2. **Repository access**: choose *Only select repositories* and pick this repository.
3. **Permissions > Repository permissions > Actions**: choose *Read and write*. Leave the rest.
4. Tap **Generate token** and copy it. It is shown once.

## Step 2. Put the service on Cloudflare

1. Tap this button (it asks you to sign in to Cloudflare, or to make a free account):

   [![Deploy to Cloudflare](https://deploy.workers.cloudflare.com/button)](https://deploy.workers.cloudflare.com/?url=https://github.com/shopifygav222/horizon-pocket-edition-ios/tree/main/packaging/mod-builds-worker)

2. Cloudflare asks to connect your GitHub account. Allow it. It keeps its own copy of this folder
   there, as a small new repository.
3. Where it asks for **GITHUB_TOKEN**, paste the token from step 1. Leave everything else as it is.
4. Tap **Deploy**. When it finishes it shows the service's address, like
   `https://horizon-pe-mod-builds.your-name.workers.dev`. Copy it.

To check it, open that address in a browser. It answers
`{"service":"horizon-pe-mod-builds","api":1}`.

## Step 3. Tell the app where the service is

1. In this repository on GitHub: **Settings > Secrets and variables > Actions > Variables**.
2. **New repository variable**. Name: `HORIZON_MOD_BUILDS_URL`. Value: the address from step 2,
   with no slash at the end.

That is all. Every app built after this has the Build button, including the builds the service
itself starts. An app built before it shows no Build button and says it has no build service.

## Good to know

- A build takes about half an hour and is kept 90 days. Asking for the same mods again within
  that time gives the same build at once, to anyone.
- The service builds at a release's tag, so it works for apps of a published release (1.1.0 or
  newer), not for test builds made from a branch.
- Limits: 6 new builds a day for one device, 300 a day for everyone, 24 mods in a build. They are
  the numbers in `wrangler.toml`.
- If you build the app from a fork, set `REPO` in `wrangler.toml` to `you/your-fork` before step 2,
  and use your fork's address in the button's link.

## If something goes wrong

| What the game says | What to do |
|---|---|
| "The build service is not set up" | Step 2 was not finished, or the token was left empty |
| "GitHub did not start the build (401)" or "(403)" | The token expired or lacks *Actions: Read and write*: make a new one (step 1) and put it in the Worker's settings on Cloudflare, under **Variables and Secrets** |
| "This app's version has no builds with mods" | The app is older than 1.1.0, or its release tag was deleted |
| "This device has started 6 builds today" | The daily limit; tomorrow there are more |
| "... can't be built into the app" | That mod changes the game's own code, which no build from the catalog does. Turn it off on the Mods page and build again |
| "The build failed at: ..." | The build itself failed. It is on this repository's **Actions** page, under **mod-build** |

To turn the service off, delete the Worker in Cloudflare's dashboard and delete the token on
GitHub. The game then says the service can't be reached; nothing else changes.

## From a computer instead

Step 2 can also be done in a terminal, in this folder, with Node.js installed:

```sh
npx wrangler login
npx wrangler secret put GITHUB_TOKEN
npx wrangler deploy
```

The second command asks for the token. The third prints the service's address.

## For developers

`node --test worker.test.mjs` runs the Worker against a pretend GitHub. Against the real one:

```sh
curl -s -X POST https://<address>/v1/build \
  -d '{"tag":"v1.1.0","mods":["zoom@1.0.0"],"device":"0123456789abcdef0123456789abcdef"}'
curl -s https://<address>/v1/build/<the key it answered with>
```
