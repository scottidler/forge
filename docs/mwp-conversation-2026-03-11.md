# MWP Conversation - 2026-03-11

Conversation about applying Model Workspace Protocol (MWP) from Jake Van Clief's paper/video to Scott Idler's workflows.

---

## Response 1: Initial Recommendation

OK, here's my recommendation.

---

### MWP Applied to Scott Idler's Workflows

### The core insight for your situation

MWP and loopr are **complementary, not competing**. MWP handles sequential, human-reviewed content workflows (docs, meeting notes, reports). Loopr handles concurrent, multi-agent code workflows. You need both.

The paper itself calls this out: MWP doesn't work for real-time multi-agent collaboration or complex branching. That's exactly what loopr does. But your *work deliverables* - the Confluence docs, postmortems, meeting notes, status reports, Jira epics - are textbook MWP territory: sequential stages, human review at each boundary, repeatable pipeline.

### Proposed workspaces (priority order)

#### 1. Meeting Notes Pipeline (highest impact given 9am-2pm meeting density)

```
meeting-notes/
  CLAUDE.md                    # Layer 0: identity
  CONTEXT.md                   # Layer 1: routing
  stages/
    01_transcript/             # Input: Google Meet transcript/recording
      CONTEXT.md               # Extract raw transcript, identify speakers
      output/                  # Clean transcript with speaker labels
    02_structure/              # Transform into structured meeting notes
      CONTEXT.md               # Apply meeting notes template
      references/              # Meeting notes template, attendee context
      output/                  # Structured notes (decisions, actions, open questions)
    03_publish/                # Format for Obsidian + extract Jira actions
      CONTEXT.md               # Render frontmatter, wiki-links, action items
      references/              # Obsidian frontmatter schema, Jira project mappings
      output/                  # Final .md for vault + action item list
  _config/
    voice.md                   # Your writing style for notes
    templates/                 # Meeting type templates (1:1, standup, planning, etc.)
```

**Why first**: You attend meetings 9am-2pm daily. Turning transcripts into vault notes with extracted action items closes the gap you identified ("meeting happened" -> "decisions captured"). This pipeline runs daily.

#### 2. Confluence Doc Pipeline (design docs, runbooks, postmortems)

```
confluence-doc/
  CLAUDE.md
  CONTEXT.md
  stages/
    01_research/               # Gather context: Jira tickets, Slack threads, existing docs
      CONTEXT.md
      output/                  # Research brief with key context assembled
    02_outline/                # Structural plan for the document
      CONTEXT.md
      references/              # Doc type template (postmortem, design doc, runbook)
      output/                  # Outline with section headers, key points per section
    03_draft/                  # Full draft following outline + voice
      CONTEXT.md
      references/              # voice.md, Tatari conventions, prior examples
      output/                  # Complete draft
    04_review/                 # Self-review against rubric
      CONTEXT.md
      references/              # Review rubric (completeness, clarity, actionability)
      output/                  # Final draft + review checklist
  _config/
    voice.md
    templates/                 # postmortem.md, design-doc.md, runbook.md, onboarding.md
```

**Why**: You write all of these doc types. The templates in `_config/` are the "factory" - set them up once, every doc follows the same standard. The review stage (04) is your guardrail: the AI checks its own output against a rubric before you see it.

#### 3. Weekly Status / Stakeholder Update

```
status-update/
  CLAUDE.md
  CONTEXT.md
  stages/
    01_gather/                 # Pull signals from Slack, Jira, meeting notes
      CONTEXT.md
      output/                  # Raw signal dump: what happened this week
    02_synthesize/             # Distill into narrative with priorities
      CONTEXT.md
      references/              # Status update template, audience context
      output/                  # Draft status update
    03_polish/                 # Tone, formatting, exec-readiness
      CONTEXT.md
      references/              # voice.md, prior status updates as examples
      output/                  # Final update ready to send
  _config/
    voice.md
    audience.md                # Who reads this, what they care about
```

#### 4. Jira Epic/Story Pipeline

```
jira-pipeline/
  CLAUDE.md
  CONTEXT.md
  stages/
    01_requirements/           # Input: rough idea, Slack thread, meeting notes
      CONTEXT.md
      output/                  # Structured requirements doc
    02_breakdown/              # Epic + stories with acceptance criteria
      CONTEXT.md
      references/              # Jira conventions, team capacity context
      output/                  # Epic description + story list (markdown)
    03_review/                 # Verify completeness, estimate sanity
      CONTEXT.md
      references/              # Review rubric, historical velocity data
      output/                  # Final ticket specs ready for Jira creation
```

#### 5. Obsidian Ingestion Pipeline (formalize what we just did manually)

```
obsidian-ingest/
  CLAUDE.md
  CONTEXT.md
  stages/
    01_extract/                # Input: URL, PDF, transcript, etc.
      CONTEXT.md               # Route by content type, extract to markdown
      output/                  # Raw markdown text
    02_summarize/              # Fabric patterns: extract_article_wisdom, etc.
      CONTEXT.md
      references/              # Pattern selection rules by content type
      output/                  # Structured summary
    03_render/                 # Frontmatter + body + cross-links
      CONTEXT.md
      references/              # Frontmatter schema, vault folder map, tag conventions
      output/                  # Final .md note ready for vault
  _config/
    frontmatter-schema.md
    folder-routing.md
    tag-conventions.md
```

**Why this is interesting**: obsidian-borg already does this in Rust code. But the MWP workspace version lets you iterate on the *quality standards* (references/) without recompiling. You could use the workspace for new content types (PDFs, meeting transcripts, podcasts) while obsidian-borg handles the high-volume automated flow (YouTube, articles).

### Where MWP does NOT apply (use loopr instead)

- **Rust CLI tool development** - branching, concurrent, needs code execution. This is loopr's domain.
- **Incident response** - real-time, not sequential. Use PagerDuty + Slack + runbooks.
- **Code review** - interactive, not pipeline-shaped. Agent-assisted but not MWP.

### Implementation strategy

1. **Start with workspace #1 (meeting notes)**. You have meetings every day, so you get daily reps. The pipeline is short (3 stages), the input is predictable (transcripts), and the value is immediate.
2. **Build the `_config/voice.md` once and share it across all workspaces.** This is the MWP `shared/` pattern - your writing style, tone preferences, formatting conventions. One file that all workspaces reference.
3. **Use your Obsidian vault as Layer 4 output.** Every workspace's final stage writes to the vault. The vault becomes both the consumption layer and the accumulated knowledge base.
4. **Review rubrics are your guardrails.** Each workspace's review stage references a rubric. The rubric IS the quality standard you mentioned - "same level of quality I would have given." Start simple, iterate as you find gaps.

### One more question

Do you want me to actually build workspace #1 (meeting notes) as a working folder structure you can start using today? I'd set it up with your voice, your meeting types, and wired to output Obsidian notes.

---

## Scott's feedback on Response 1

> one thing to know about Loopr is it is my attempt at doing multi agent task coordination. This is an unsolved problem in the industry. So to assume that Loopr just totally handles a whole class of problems. It does not... yet. I think we missed a bit of research, especially about some technology or approach we want to take with our software and also writing and clear and concise PRD, TechSpec, etc.
>
> one thing I wasnt sure of for the author's youtube example and maybe your tailored version spelled out above. do I have to crawl all of these different directories in this nested hierarchy? that is less than apetizing for the way I work, often flitting beween repo to repo all day long.
>
> thoughts?

---

## Response 2: Addressing Loopr and Directory Crawling

Two really important points. Let me address both.

### On loopr

You're right - I oversimplified. Loopr is your *attempt* at solving multi-agent coordination, not a solved problem you can point MWP away from. That changes the picture: MWP-style workspaces could actually help you *build* loopr better, not just handle the stuff loopr doesn't. Research stages, design docs, PRDs - those are sequential, human-reviewed workflows that feed into the code you write.

I should have included workspaces like:

- **Technical research** - investigating an approach before committing to it (e.g., "how do other systems handle agent state recovery?" -> structured findings -> design implications)
- **PRD / TechSpec pipeline** - problem statement -> user stories -> requirements -> polished PRD with review rubric
- **Design doc pipeline** - you already have a design doc convention (`docs/design/YYYY-MM-DD-*.md`) across your repos. A workspace could standardize the flow from research -> outline -> draft -> spec-review

### On the directory crawling problem

This is a legitimate friction concern. The author's workspace structure assumes you're *camped out in one workspace* running a pipeline end to end. Your actual workflow is:

> flitting between repo to repo all day long

You're not sitting in `meeting-notes/stages/02_structure/` editing files. You're jumping from loopr to obsidian-borg to taskstore to a Tatari repo to Slack to Confluence and back. The nested hierarchy works for someone whose primary workspace IS the pipeline. For you, it's overhead.

**Two possible adaptations:**

**Option A: Flat workspaces, not nested stages.** Instead of `stages/01_research/CONTEXT.md`, you have a single workspace folder with numbered markdown files that serve as both the stage contracts AND the outputs:

```
meeting-notes/
  CLAUDE.md
  00-routing.md          # what type of meeting, which template
  01-transcript.md       # raw input (pasted or fetched)
  02-structured.md       # AI output: structured notes
  03-final.md            # reviewed, ready for vault
  _config/
    voice.md
    templates/
```

Still sequential, still staged, but the "stage" is a file, not a directory. You `ls` the workspace and see the whole pipeline state at a glance. Claude Code reads the CLAUDE.md, sees the numbered files, knows what to do.

**Option B: Workspaces live inside your existing repos, not as standalone folders.** Your repos already have `docs/design/` directories. A workspace could be a `.workspace/` or `docs/workspace/` folder *inside* the repo it serves:

```
~/repos/scottidler/loopr/
  docs/
    design/               # existing design docs
    workspace/            # MWP workspace for this repo
      CLAUDE.md           # "this workspace helps research and write design docs for loopr"
      research.md         # stage 1 output
      outline.md          # stage 2 output
      draft.md            # stage 3 output
      _config/
        techspec-template.md
        voice.md -> ~/repos/scottidler/obsidian/templates/voice.md  # symlink, shared
```

When you `cd ~/repos/scottidler/loopr` and start a Claude Code session, the repo's CLAUDE.md can reference the workspace. No extra navigation - you're already in the repo you're working on.

**Option C: Hybrid - obsidian-borg style.** You already solved this pattern: obsidian-borg is a CLI daemon that takes an input (URL), runs a pipeline internally, and writes output to the vault. The "workspace" is the code, not a folder you navigate. For meeting notes, you could build a similar tool: `borg-notes ingest <transcript>` and it runs the stages programmatically, writing to the vault.

This is closer to how you actually work - fire a command, review the output, done. The MWP principles (staged context, reference/working separation, review gates) still apply, they're just encoded in a CLI tool rather than a folder you crawl through.

**My instinct is Option B for research/writing workflows (they live where you already are) and Option C for high-frequency repeatable pipelines (meeting notes, ingestion) where you'd build a tool.** But this is your call - what feels more natural?

---

## Scott's feedback on Response 2

> ```
> ~/repos/scottidler/claude-cost-usage saidler in 🌐 desk in on  main is 📦 v0.3.15 via 🦀 v1.94.0 on ☁️   took 2s
> ❯ ls-github-repos scottidler | wc -l
> 108
>
> ~/repos/scottidler/claude-cost-usage saidler in 🌐 desk in on  main is 📦 v0.3.15 via 🦀 v1.94.0 on ☁️   took 2s
> ❯ ls-github-repos tatari-tv | wc -l
> 269
> ```
>
> hopefully you see how asinine that option B suggestion was. What about a third. I write a lot of software. Alot of rust cli software. Is it possible to model this inside a rust cli program. subcommands and flags are used to power this MWP paradigm. Remember I AM a technologist/engineer/builder. I built otto, a Makefile replacement and now use it regularly in my own and work projects.
>
> Thoughts?

---

## Response 3: MWP as a Rust CLI

Fair point - 377 repos. Sprinkling `.workspace/` folders across them is absurd. I should have thought that through.

And yes - your instinct is exactly right. You already proved this pattern works: **obsidian-borg IS an MWP pipeline encoded as a Rust CLI**. It has stages (normalize -> dedup -> classify -> fetch -> summarize -> tag -> render -> write -> log), reference material (config, templates, voice), working artifacts (the content being processed), and human review (you look at the note in Obsidian). It just doesn't have folders - it has functions.

The MWP *principles* matter. The folder structure is Jake's implementation detail for non-engineers who can't write code. You can.

So the question becomes: **what would a general-purpose MWP-as-CLI look like for you?**

A few directions to think about:

### Direction 1: Extend obsidian-borg into a broader tool

obsidian-borg already handles youtube, articles, and now (manually) PDFs. What if it also handled:
- Meeting transcripts (`borg ingest --type transcript meeting.vtt`)
- Slack thread digests (`borg ingest --type slack-thread <url>`)
- Research pipelines (`borg research "how do other systems handle agent state recovery"`)

The vault is the universal output layer. The subcommands define the pipeline shape. The config defines the reference material (voice, templates, routing rules). This is MWP with `cargo run` instead of `cd stages/01_research`.

### Direction 2: A new tool - something like `forge` or `pipeline`

A general-purpose stage runner where pipelines are defined in config (like otto tasks are defined in `.otto.yml`), each stage calls fabric patterns or shell commands, and outputs flow between stages:

```yaml
# ~/.config/forge/forge.yml or per-repo .forge.yml
pipelines:
  meeting-notes:
    stages:
      - name: extract
        input: "$1"  # transcript file/url
        run: "pdftotext -layout $input - || cat $input"
      - name: structure
        pattern: "meeting_notes"  # fabric pattern
        references:
          - "~/.config/forge/voice.md"
          - "~/.config/forge/templates/meeting-notes.md"
      - name: render
        pattern: "obsidian_note"
        output: "~/repos/scottidler/obsidian/Work/Meetings/"

  techspec:
    stages:
      - name: research
        pattern: "extract_article_wisdom"
      - name: outline
        pattern: "create_outline"
        references:
          - "~/.config/forge/templates/techspec.md"
      - name: draft
        pattern: "write_techspec"
      - name: review
        pattern: "review_against_rubric"
        references:
          - "~/.config/forge/rubrics/techspec-rubric.md"
```

```bash
forge run meeting-notes ./transcript.vtt
forge run techspec --input "loopr agent state recovery design"
forge run techspec --resume  # pick up from where you left off (last stage output)
```

The stage outputs are files in a temp/working directory (or a named run directory if you want to review intermediate state). Each stage reads the previous stage's output + its references, pipes through fabric or a custom command, writes output. Human review gates are opt-in (`--review-at 2` or `review: true` in the stage config).

### Direction 3: This is already what otto could become

Otto is your task runner. Pipelines are task chains. You already have the execution model. The missing piece is the *context plumbing* - feeding stage outputs into stage inputs, loading reference material, and routing through fabric patterns.

---

My gut says **Direction 2** is the sweet spot. It's a new tool purpose-built for this (clean separation from obsidian-borg which is content-ingestion-specific), it's config-driven (like otto), it's a Rust CLI (your wheelhouse), and it encodes MWP principles without MWP's folder-crawling. You'd `scaffold` it and be productive in a day.

But I want to hear your reaction before going further. Does any of these resonate, or are you thinking something different?

---

## Scott's feedback on Response 3

> not everyone is bought into Brad's conductor approach, I am not. But maybe because it is built for a specific usecase which is not me.
>
> thought, instead of folders what about git worktrees. These are relatively new, and are kinda catching fire in other ways. Also Git Butler recently released has first class support for them. note a git repo is a specialized kind of db
>
> another idea, what about an actual db? I am concerned about the folder and file sprawl navigating around through it. Probably because I use the terminal and neovim most of the time, not an ide like vscode or cursor. I guess I could navigate around in Obsidian. I hadnt thought of that, until now.
>
> Thoughts?

---

## Interlude: Conductor comparison

Before responding, I explored `~/repos/tatari-tv/conductor` (Brad Yinger's Claude Code plugin). Conductor already implements many MWP principles: sequential stages with human review (PRD -> tech-plan -> epics -> tickets -> implement), a `thoughts/` directory as persistent reference material, markdown outputs as edit surfaces, and pause-between-phases review gates. However, it's purpose-built for the software development lifecycle and doesn't cover meeting notes, content ingestion, status updates, or non-dev writing pipelines.

---

## Response 4: Worktrees, Databases, and Obsidian as Navigation Layer

Three ideas on the table. Let me be direct about each.

**Git worktrees** - great for what loopr already uses them for (isolated code branches for parallel implementation). Not great as a pipeline state store. A worktree is an execution environment, not a database. You'd end up with `git worktree list` showing dozens of abandoned pipeline runs alongside your actual code worktrees. The metaphor doesn't fit.

**Actual database** - you already built this. TaskStore is JSONL + SQLite with git-native durability, merge drivers, and queryable indexes. You solved the storage problem. The question isn't "should I use a database" but "should I extend TaskStore to hold pipeline state."

**Obsidian as the navigation layer** - this is the most interesting thing you said. You just surprised yourself with it. And it's worth sitting with.

Here's why: Obsidian IS a database with a good UI. It has full-text search, graph view, backlinks, Dataview queries, tags, frontmatter filtering. You already have ingestion pipelines feeding into it. You navigate it daily. And critically - **you don't navigate Obsidian in the terminal**. It's the one place where folder structure and file sprawl don't bother you because you're using a purpose-built interface.

So what if the architecture is:

```
                    CLI (Rust)
                   /    |     \
            fabric   pdftotext  yt-dlp  ...  (stage executors)
                   \    |     /
                  TaskStore (state)
                   /         \
            JSONL+SQLite    Obsidian vault
            (pipeline        (human-readable
             records,         output, review,
             stage state,     cross-linking,
             resume info)     graph navigation)
```

- **TaskStore** tracks pipeline runs, stage state, inputs, outputs, resume points. You query it from the CLI: `forge status`, `forge resume`, `forge history meeting-notes`.
- **Obsidian** is where finished (and in-progress review) outputs land. You browse, edit, cross-link, and query there. Dataview gives you dashboards ("all techspecs in review", "meeting notes this week").
- **The CLI** is how you kick things off and advance stages. `forge run meeting-notes transcript.vtt` starts a pipeline, stores state in TaskStore, writes stage outputs, and when a stage needs human review, the output appears in your vault with a `status: review` frontmatter field.
- **Fabric** (and other tools) are the stage executors - called by the CLI, not by you directly.

This means:
- No folder sprawl to navigate in terminal
- `forge ls` shows your active pipelines (queries TaskStore/SQLite)
- Review happens in Obsidian where you already live
- Pipeline state survives crashes (TaskStore's whole point)
- Git-native (JSONL commits)
- You can query across pipeline runs ("what patterns came up in my last 5 techspec research stages?")

The part I'd push back on is building this as a *general-purpose* tool right away. obsidian-borg already works for content ingestion. The highest-value new pipeline is meeting notes (daily volume, clear input format, clear output target). I'd build the meeting notes pipeline first, with TaskStore as state and Obsidian as output, and let the general-purpose abstractions emerge from real usage rather than designing them upfront.

---

## Scott's feedback on Response 4

> the reality is 1) I dont take great notes now. very rarely hand written, adhoc in linux text file editor, files never saved, just leave the thing open and running with multiple tabs. 2) until I get access to transcripts either via Gemini or via Granola, this is not the one to focus on in my opinion

(Meeting notes pipeline is blocked on transcript access - dependency, not starting point.)

> I think we are in the figure out the right approach before picking a pipeline. Still thinking of how heavy this folder structure is, we could carry it around like a briefcase that we unfurl into a location with symlinks. setup shop, do our work, and then fold everything back up. this would allow us to operate in any of the numerous repos, and other areas of the filesystem without leaving giant bloat and boilerplate around. thoughts?

---

## Response 5: The Briefcase Pattern

That's a really clean metaphor. A portable workspace that deploys into your current location and retracts when done. Like `nix-shell` or `direnv` but for MWP context.

Something like:

```bash
# You're in ~/repos/scottidler/loopr working on a design doc
forge init research        # deploys workspace scaffolding via symlinks
                            # into current dir or a .forge/ subdirectory

# ... do the work, stages advance ...

forge pack                  # retracts symlinks, stashes outputs back
                            # to central store (TaskStore + vault)
                            # leaves no trace in the repo
```

What actually happens on `forge init research`:

- The **templates, voice.md, rubrics, stage contracts** (Layer 3 / reference material) live in one central location (`~/.config/forge/` or a dedicated repo)
- Symlinks are created pointing into the current directory (or `.forge/`)
- Working artifacts (your actual outputs) are real files, not symlinks
- On `forge pack`, outputs get committed to TaskStore and/or written to vault, symlinks are removed

The central store is the "briefcase." It contains all your pipeline definitions, reference material, and templates. `forge init` unpacks what you need. `forge pack` collects the results and cleans up.

What I like about this:
- **Zero repo pollution** - symlinks vanish on pack, no `.workspace/` folders committed
- **Works anywhere** - any repo, any directory, `~/scratch`, doesn't matter
- **Reference material stays DRY** - one `voice.md`, one set of templates, symlinked everywhere
- **Outputs land in the right place** - TaskStore for state, vault for notes, `docs/design/` for design docs (configurable per pipeline)
- **Git-friendly** - you can `.gitignore` the symlinks (or `.forge/`) so they never get committed

The questions that sharpen it:

1. **Where does the briefcase live?** A single repo (`~/repos/scottidler/forge`) with all pipeline definitions and reference material? Or `~/.config/forge/`? The repo approach means it's version-controlled and you can iterate on pipeline definitions the way you iterate on code.
2. **Does `forge init` drop symlinks in the repo root or in a `.forge/` subdirectory?** Subdirectory is cleaner (one `.gitignore` entry), but root is more visible.
3. **What about concurrent briefcases?** If you `forge init research` in loopr, then switch to taskstore and `forge init research` there too - does each location get its own instance? TaskStore can track these as separate pipeline runs keyed by `(pipeline_type, working_dir)`.
4. **Is `pack` manual or automatic?** Could be automatic on completion of the final stage, with manual `forge pack` as an escape hatch for abandoned runs. `forge ls` shows all deployed briefcases across your filesystem.

This feels like it's converging. The briefcase pattern solves the "I don't want folder sprawl in 377 repos" problem while keeping MWP's principles intact.

---

## Scott's feedback on Response 5

> lets sharpen
> 1. definitely in a single repo, what you suggested for now
> 2. adds .forge/ to the repo root; adds the .forge to .gitignore first, collisions cause an error and halt
> 3. yes they get their own instance, maybe some forge command to connect them somehow; waving hands
> 4. forge commands are manual for now, including pack/unfurl whatever we call it. i dont know what you mean "with manual forge pack as an escape hatch for abandoned runs". yes to forge ls.
> if I understand MWP correctly (and remind me what that stands for) this is a pipeline with artifacts running through it. this part has always given me pause, but maybe it is just me
> 1. when we pack up, what happens to these artifacts? like we are midway through OR we are we are finished with a final product, what happens to the intermediates? were they stored locally? store externally? certainly not in side the forge repo because that is focused on all of the knowledge scaffolding
>
> I thought I had more questions, cant remember them

---

## Response 6: Artifact Lifecycle and the Three Categories

MWP = **Model Workspace Protocol** (Jake Van Clief's paper).

Your artifact question is the central design question. Let me think through it clearly.

There are three categories of stuff:

| Category | What it is | Where it lives | Examples |
|---|---|---|---|
| **Factory** | Pipeline definitions, templates, voice, rubrics, stage contracts | `~/repos/scottidler/forge/` (the briefcase repo) | `pipelines/research.yml`, `references/voice.md`, `rubrics/techspec-rubric.md` |
| **Intermediates** | Stage outputs mid-pipeline | `.forge/` while active, **TaskStore** when packed | Research notes from stage 1, outline from stage 2 |
| **Finals** | Finished product | Configured destination per pipeline | Design doc -> `docs/design/`, Obsidian note -> vault, Confluence page -> Confluence |

So on `forge pack`:

1. **Final output** gets written to its destination (already happened at the last stage, or happens now)
2. **Intermediates + pipeline state** get serialized into TaskStore - the JSONL records capture what stage you're at, what each stage produced, inputs, timestamps
3. **`.forge/` directory gets removed**, `.gitignore` entry gets removed
4. The repo is clean

On `forge unfurl` (resume a packed-up pipeline):

1. TaskStore is queried for the pipeline run keyed by `(pipeline, original_working_dir)` or a run ID
2. `.forge/` is recreated, intermediates are restored from TaskStore
3. You pick up at whatever stage you left off

If you **finished** the pipeline and packed - the intermediates are still in TaskStore as history, but `.forge/` is gone and the final product is in its destination. You can `forge history` or `forge show <run-id>` to inspect old intermediates if you ever need to trace how a final product was built (this is the "observability" MWP talks about).

If you **abandoned** mid-pipeline and packed - same thing, but the run is marked incomplete. `forge ls` shows it. `forge unfurl <run-id>` can resurrect it anywhere (even a different directory if the context has moved).

On your pause about "pipeline with artifacts" - I think the concern is accumulation. Where does all this stuff pile up? The answer is: **TaskStore is the attic**. It's append-only JSONL, git-committed, queryable, but you never navigate it directly. You interact through `forge ls`, `forge show`, `forge history`. The artifacts are there if you need them, invisible if you don't.

The only thing that's ever "in your face" is:
- `.forge/` while you're actively working (and it's in `.gitignore`)
- The final output in its destination (vault, docs/, Confluence)
