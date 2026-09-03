# Brief: rewrite the herdup landing page around the new storyline

You are updating an existing, **live, public** marketing page. Everything here is
in `D:\work\herdr_automation\site\` — `index.html`, `styles.css`, `scroll.js`.
It deploys to Railway automatically on push to `main` when `site/**` changes.

## The single hardest rule

**Do not describe anything that does not exist yet as though it ships today.**

The Client channel, the daemon, the kanban board, the messaging integrations —
**none of it is built.** It is a design, agreed today, not a feature. The current
page is scrupulous about this (it labels macOS and Linux "built, not yet
exercised" because nobody has run them). Hold that line. Anything from the
"where this is going" section below must be visibly marked as direction, not
capability — its own section, its own visual treatment, the words "not built
yet" or "planned" doing real work.

If you are unsure whether something exists, assume it does not and ask.

## What exists today (safe to describe as real)

- A desktop launcher (Windows tested; macOS/Linux built by CI, never run).
- You pick a project folder and a team size (1/2/4/6), add or remove teammates.
- It builds the herdr layout, starts each agent CLI, waits until each is
  genuinely at its prompt, and hands each its briefing. ~20 seconds.
- Then it gets out of the way and gives you the terminal.
- 24 supported agent CLIs, listed in a `registry.toml` you can edit.
- Warns before starting if the folder has no version history or uncommitted work.
- Several projects run side by side, each in its own herdr workspace.
- It will not type into an agent sitting at an approval dialog.

## Where this is going (mark clearly as NOT BUILT)

The narrative, in the author's own words:

> herdr is a tmux-like client where you can run multiple terminal instances, but
> it gives full signal control to the CLI app, which makes inter-terminal
> communication possible. Run Claude in three panes and all three can talk.
>
> herdup builds that workflow — and adds a special "channel" called **client**,
> which you connect to WhatsApp, Telegram, Slack, email, Google Docs, Discord.
> It takes everything sent to it and gives it to the PM, which turns whatever
> arrives into something understandable, executable and actionable for the
> agents. They all run autonomously, and what comes back to the client is an
> executive summary: project status, the kanban board, and the conversation
> between the PM and the agents, like Slack.

Design decisions already settled, if useful for accuracy:

- The Client is deliberately **not** an AI. It is a deterministic pipe and a
  renderer. The PM is the only thing that thinks.
- A small local daemon holds the state (SQLite) and is the only component that
  talks to herdr. Agents reach it over a unix socket with plain HTTP.
- Delegation routes through that daemon, so the Slack-style thread is a
  by-product of the real traffic rather than a log someone has to maintain.

## The ideology — this is the part that matters most

Write this like someone with a point of view, not a feature list.

The argument: one AI agent is a better autocomplete. A *team* of them is a
different kind of thing — but almost nobody gets to find out, because the setup
cost is brutal. Four terminals, four sets of permission flags, the same trust
prompt four times, briefing each one by hand. People quit before the interesting
part.

And the deeper point: **a vibe coder's scarce resource is attention, not
typing.** The bottleneck was never how fast you write code — it is how much
context you can hold while four things happen at once. Every tool so far has
made the typing faster and left the attention problem alone. The direction here
is the opposite: you say what you want, in the app you already have open, and
what comes back is a summary a human can actually hold in their head. You stay
the client. The team does the standing around.

Be honest that this is opinionated and unproven. Do not oversell.

## Constraints

- Keep the existing visual language: Space Grotesk, warm amber accent
  (`--acc`), hairlines over boxes, light and dark both on tokens.
- Keep the scroll narrative (GSAP + three.js in `scroll.js`): the point field
  scatters, gathers into a herd, resolves into four lanes. It is the product's
  name doing work. You may extend the beats; do not delete the idea.
- Keep the real screenshots. They are captures of the built app, not mockups.
  Do not add invented UI or renders of screens that do not exist.
- Keep `Requirements`, `Supported tools`, `Download`, `Support` — that content
  is accurate and load-bearing.
- Do not touch anything outside `site/`.
- **Do not commit, and do not push.** Leave the working tree dirty and report
  what you changed. This page is public; a human reviews before it deploys.

## Check your work

Serve it and look at it before you report:
`cd site && node server.js` then open http://localhost:3000 — check the console
for errors, scroll the whole page, and check it at a narrow width too.
