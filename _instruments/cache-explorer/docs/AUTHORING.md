# Authoring guided modules

A guided module is one JSON file in `_instruments/cache-explorer/modules/`, named by its id, plus a row in `modules/index.json`. This page is the whole format: every field, every step kind, every predicate, the markdown a module may use, the errors you will see, and how to add a module.

The core reads and checks modules (`Module::parse`); the page only draws what it returns. So the rules below hold whatever is on screen, and a mistake is an error at test time, not a surprise in the browser.

## Contents

- [The file](#the-file)
- [The preset](#the-preset)
- [The program](#the-program)
- [Steps](#steps)
- [Callouts](#callouts)
- [Predicates](#predicates)
- [Show me and then](#show-me-and-then)
- [Text](#text)
- [Validation](#validation)
- [A worked example](#a-worked-example)
- [Adding a module](#adding-a-module)

## The file

| Field         | Type    | Notes                                                                                           |
|---------------|---------|-------------------------------------------------------------------------------------------------|
| `id`          | string  | Two digits, a hyphen, then lower-case letters, digits and hyphens: `04-tracking-lru`. It is the file's name without `.json`. The picker lists modules in `index.json`'s order, which by convention follows the numbers. |
| `title`       | string  | Plain text, not empty.                                                                          |
| `description` | string  | One line under the title in the picker. Inline markdown (see [Text](#text)).                     |
| `advanced`    | boolean | `true` if the module needs Advanced mode: a write policy or allocate policy other than write-back with write-allocate, or timed mode. |
| `preset`      | object  | The configuration the module starts from. `preset.advanced` must equal `advanced`.               |
| `program`     | object  | Optional. A program loaded with the preset. See [The program](#the-program).                    |
| `steps`       | array   | At least one step.                                                                              |

A module that is not advanced uses basic configurations only, and so does every configuration its ops switch to: write-back, write-allocate, untimed.

Any other key is an error, with the keys that are allowed listed in the message.

## The preset

The preset is a full configuration, the same object a saved run holds.

| Field                    | Values                                                                 |
|--------------------------|------------------------------------------------------------------------|
| `addressBits`            | 8 to 32.                                                               |
| `wordBytes`              | 1, 2, 4 or 8.                                                          |
| `cache.sizeBytes`        | A power of two.                                                        |
| `cache.lineBytes`        | A power of two, at least one word, at most 16 words.                   |
| `cache.associativity`    | Ways per set: 1 is direct-mapped; the line count is fully associative. At most 64 lines in all. |
| `cache.replacement`      | `"lru"`, `"fifo"` or `"random"`.                                       |
| `cache.writePolicy`      | `"writeBack"` or `"writeThrough"`.                                     |
| `cache.allocatePolicy`   | `"writeAllocate"` or `"writeNoAllocate"`.                              |
| `timing`                 | Optional. `{ "hitCycles": n, "missCycles": n }`, each 1 to 1,000. Its presence turns timed mode on. |
| `seed`                   | The seed for random replacement, up to 2^53 - 1.                       |
| `advanced`               | The module's `advanced`.                                               |
| `memoryInit`             | Optional. `"zero"` (the default) or `"pattern"`, where the word at address `a` reads `(a / wordBytes * 13 + 5) mod 97`, so a fill brings in visible values. |

The preset is checked as any configuration is; a bad one reports the configuration's own message.

## The program

```json
"program": { "source": "R 0x000\nR 0x080", "format": "trace" }
```

`format` is `"trace"` or `"asm"`. A trace is one access per line: `R addr` for a load and `W addr value` for a store. Assembly is the text the Program drawer takes. The program is checked under the preset and under every configuration the module switches to, because a new run keeps the program; if it does not fit one of them, the error names it. Without a program a module is driven by hand with Issue.

## Steps

Every step has a `kind`, a `text`, and optionally a `callout`. The kinds differ in what moves the module on.

| Kind       | Extra fields                                         | Moves on when                                                  |
|------------|------------------------------------------------------|----------------------------------------------------------------|
| `explain`  | none                                                 | The student presses Next.                                      |
| `instruct` | `expect` (predicate), `hint?`, `showMe?`             | The student does something and `expect` holds. See below.      |
| `predict`  | `options`, `answer`, `explain`, `hint?`, `then?`     | The student picks the right option and presses Check, then Next. |
| `observe`  | `until` (predicate)                                  | The student moves the timeline to a place where `until` holds. |

A field that does not belong to the step's kind is an error (``unexpected field `until` for step `instruct` ``), and so is a missing one.

**instruct.** The step completes when three things are true together: the cursor is at the end of an entry, an op was made since the step began (an undone op counts), and `expect` holds. So a step whose `expect` is already true on entry waits for the student to act. `hint` is shown under the step text. `showMe` is the list of ops the Show me button applies; leave it out and the button is not drawn.

**predict.** `options` has at least two entries and `answer` is the index of the right one, counting from 0. The student must answer before anything else: while a prediction is open, every op is refused and the timeline cannot move forward past where the prediction began (Undo and moving back are allowed). A wrong answer shows `Not quite.` and the `hint`, or `explain` when there is no hint; it can be changed and checked again. After a wrong answer, Show me appears and reveals the answer. A right answer shows `Correct.` and `explain`. `then`, if given, is applied once the question is answered, from the state the question asked about, so the student sees the result.

**observe.** `until` is checked wherever the cursor is, not only at the end of the log. Show me moves the cursor to the first place in the run where `until` holds, counting only ops made since the step began. If it holds nowhere, Show me does nothing. Use an `event` predicate here to ask for a micro-step, such as the tag compare of the first access.

When the last step is complete the panel shows the done screen, with Restart.

## Callouts

`callout` draws an outline around one region while the step is current. It is one of:

- a region name: `"addressStrip"`, `"cache"`, `"memory"`, `"log"`, `"stats"`, `"config"`, `"cpu"` or `"transport"`;
- a cache cell: `{ "set": 2 }` for a whole set, `{ "set": 2, "way": 0 }` for one way.

A set or way that the configuration in force at that step does not have is an error.

## Predicates

A predicate is an object with exactly one of the keys below, plus that key's own companions. Two keys at once is an error (``a predicate needs exactly one of `way`, `line`, ...; found way, line``), as is a companion that does not belong (`` `rank` is not allowed in a `line` predicate (allowed: present, dirty)``). Every field named in a predicate must match; fields left out are not checked.

| Predicate    | Shape                                                                                    | True when                                                              |
|--------------|------------------------------------------------------------------------------------------|------------------------------------------------------------------------|
| `way`        | `{ "way": { "set": n, "way": n }, "valid"?, "tag"?, "dirty"?, "rank"? }`                  | Every given field equals the way's. `tag` is the tag value, not the line. `rank` is the place in the replacement order, and needs LRU or FIFO and more than one way. |
| `line`       | `{ "line": { "addr": a }, "present"?, "dirty"? }`                                         | A valid way holds the line containing `addr`. `present` defaults to `true`; `dirty: true` implies present, and `present: false` with `dirty: true` is an error. |
| `memory`     | `{ "memory": { "addr": a }, "equals": v }`                                                | Memory (not the cache) holds `v` at `addr`. `addr` must be word-aligned and `v` must fit the word. |
| `counter`    | `{ "counter": name, "op": "==" \| ">=" \| "<=", "value": n }`                             | The comparison holds for the counter at the cursor.                    |
| `register`   | `{ "register": "r0" ... "r7", "equals": v }`                                              | The register holds `v`.                                                |
| `all`        | `{ "all": [p, ...] }`                                                                     | Every predicate holds. An empty list is an error.                      |
| `any`        | `{ "any": [p, ...] }`                                                                     | At least one holds. An empty list is an error.                         |
| `not`        | `{ "not": p }`                                                                            | `p` does not hold.                                                     |
| `event`      | `{ "event": { "kind": k, "entry"?: n } }`                                                 | The event at the cursor is of kind `k` and, if `entry` is given, belongs to entry `n` of the run (from 1). |
| `lastAccess` | `{ "lastAccess": { "kind"?: "read" \| "write", "addr"?: a, "outcome"?: "hit" \| "miss" } }` | The latest completed access at or before the cursor, made since the step began, matches every given field. |
| `config`     | `{ "config": { "sizeBytes"?, "lineBytes"?, "associativity"?, "replacement"?, "writePolicy"?, "allocatePolicy"?, "timed"? } }` | The configuration of the run at the cursor matches every given field. `timed` is a boolean. |

`event`, `lastAccess` and `config` test things the machine's state alone does not show: the micro-step at the cursor, what the student just did, and the configuration. `misses == 1` is true at the tag compare of the first access and at every micro-step after it; `{ "event": { "kind": "miss", "entry": 1 } }` is true only at the miss. `lastAccess` fires only for an access made after the step began, so a student who loaded the address before the step started does not complete it. `config` is how a step asks the student to change a setting.

Event kinds: `decode`, `hit`, `miss`, `victim`, `writeBack`, `writeThrough`, `fill`, `write`, `rank`, `memWrite`, `stall`, `complete`, `edit`, `error`, `evict`, `setup`.

Counters: each counts what its name says, at the cursor.

| Counter         | Counts                                                                     | Can move when                          |
|-----------------|----------------------------------------------------------------------------|----------------------------------------|
| `accesses`      | Loads and stores looked up in the cache.                                    | always                                 |
| `loads`         | Loads looked up.                                                            | always                                 |
| `stores`        | Stores looked up.                                                           | always                                 |
| `hits`          | Accesses that found their line.                                             | always                                 |
| `misses`        | Accesses that did not.                                                      | always                                 |
| `evictions`     | Lines that left the cache to make room.                                     | always                                 |
| `writebacks`    | Dirty lines written back to memory.                                         | write-back                             |
| `writethroughs` | Stores written through to memory under write-through. | write-through |
| `writearounds`  | Stores that missed under write-no-allocate and went straight to memory.     | write-no-allocate                      |
| `fills`         | Lines brought in from memory.                                               | always                                 |
| `cycles`        | Cycles, in timed mode.                                                      | timed mode only                        |
| `compulsory`    | Misses on a line never filled in this run.                                  | always                                 |
| `capacity`      | Misses a fully associative cache of the same size would also have.          | always                                 |
| `conflict`      | The remaining misses, caused by where lines may go.                         | always                                 |

Only `cycles` is checked: a predicate on it is an error where the configuration in force is not timed. The other restrictions are in the "Can move when" column for your information; a test of a counter that cannot move can never be true, so check the step's own walk-through (see [Adding a module](#adding-a-module)).

Numbers and addresses:

- An address is a JSON number or a string of `0x` and one to eight hex digits (`"0x0A4"`), which is easier to read. Everywhere an address appears in module JSON (`line.addr`, `memory.addr`, `lastAccess.addr`, an issue's `addr`, a memory edit's target, a way edit's `line`) either form is accepted; it must fit `addressBits`.
- `equals` is a JSON number, read as a word, exact up to 2^64 - 1.
- The JSON reader limits nesting to 128 levels for the whole file, so a predicate nests about 120 deep (an `all` or `any` level counts twice). Going past it gives "recursion limit exceeded".

## Show me and then

`showMe` (on an instruct step) and `then` (on a predict step) are lists of ops. Each is applied as an op of its own, so the log shows them as if the student had made them, and Undo undoes them one at a time. They are checked together: if the state refuses one, none is applied and the panel says why (a full session, for instance).

| Op        | Shape                                                                               |
|-----------|-------------------------------------------------------------------------------------|
| `issue`   | `{ "op": "issue", "kind": "read" \| "write", "addr": a, "value"?: v }`                |
| `step`    | `{ "op": "step" }` executes one instruction.                                              |
| `stepAccess` | `{ "op": "stepAccess" }` executes instructions until one memory access completes.                                   |
| `run`     | `{ "op": "run", "n": n }` runs `n` instructions, 1 to 1,000.                          |
| `config`  | `{ "op": "config", "config": { ...a full configuration... } }` starts a new run with it, as "Reset and apply" does. |
| `program` | `{ "op": "program", "source": "...", "format": "asm" \| "trace" }` replaces the program. |
| `edit`    | `{ "op": "edit", "target": t, "patch": p }`, where `t` is `{ "kind": "memory", "addr": a }` or `{ "kind": "way", "set": n, "way": n }`, and `p` is `{ "kind": "memory", "value": v }` or `{ "kind": "way", "valid"?, "line"?, "data"?, "dirty"?, "rank"? }`. |

An instruct step whose `showMe` contains a `config` op must say so in `expect`: it needs a `config` predicate (itself, or inside `all` at any depth) that the new configuration meets and the one before it does not. Without that the step could never be told apart from "nothing changed". A switch no `config` predicate can tell apart, such as a reset to the same configuration, belongs in a predict's `then`, not in an instruct's `showMe`. Later steps are checked against the configuration in force after the last `config` op before them.

An instruct step that asks the student to change a setting names the target in `expect` with a `config` predicate, so a change made by hand completes it as well as one made by Show me.

## Text

Step `text`, `explain`, each of `options`, `hint`, and the module `description` allow inline markdown only:

- `` `code` ``: one backtick each side, not empty; the content is literal.
- `*emphasis*`: one asterisk each side, not starting or ending with a space, holding no code.

The text is one paragraph, and not empty. The rest of markdown is refused, with the character it starts at: a line break; at the start, a heading, a block quote, a list item, an indented code block and a rule; anywhere, `**`, an underscore outside code, `~~`, a link, an image, HTML, an entity (`&amp;`) and a backslash escape. Titles are plain text.

Write addresses and field names in code (`` `0x0A4` ``, `` `index` ``), and a term the student is meeting for the first time in emphasis (`*write-back*`).

## Validation

`Module::parse` returns the first error it finds, with a JSON path:

```text
steps[2].until.event.entry: entry must be at least 1.
steps[1].options[1]: HTML at character 1 is not supported; module text allows only `code` and *emphasis*.
```

JSON that does not have the format's shape gives serde's message, with its line and column, for example ``unknown field `vaild`, expected one of `way`, ... at line 1 column 32``.

What is checked:

- the id has the form above, the title is not empty, and the text uses only the inline subset;
- the preset validates, and `preset.advanced == advanced`; a module that is not advanced uses basic configurations only;
- the program parses under the preset and under every configuration the module switches to;
- `steps` is not empty; a predict has at least two options and an `answer` that is one of them; `showMe` and `then`, when given, are not empty;
- sets, ways, tags and ranks are in range for the configuration in force at the step; rank predicates only where the replacement is LRU or FIFO with more than one way; addresses fit `addressBits`, and the addresses of `memory` and `lastAccess` are aligned; values fit the word;
- `cycles` only where the configuration in force is timed; `entry` is at least 1; no empty `all` or `any`; a `line` that is not present is not dirty;
- the ops pass the structural checks every op does (an issue's address and value, a program that parses, a run count in range, an edit's target and patch), and their `config` ops validate;
- callouts name a set and way the configuration in force has;
- an instruct step's `showMe` that changes the configuration has the `config` predicate described above.

`index.json` is checked as a whole: its ids are unique and of the form above, titles are not empty, and descriptions use only the inline markdown subset.

## A worked example

This is `modules/03-conflict-misses.json`, as shipped. It has a program, a prediction, instruct steps that end in a counter check and in a `config` check, Show me ops of two kinds, and a callout of each shape.

```json
{
  "id": "03-conflict-misses",
  "title": "Conflict misses",
  "description": "Two lines that share a set push each other out of a *direct-mapped* cache. A second way fixes it.",
  "advanced": false,
  "preset": {
    "addressBits": 12,
    "wordBytes": 4,
    "cache": {
      "sizeBytes": 128,
      "lineBytes": 16,
      "associativity": 1,
      "replacement": "lru",
      "writePolicy": "writeBack",
      "allocatePolicy": "writeAllocate"
    },
    "seed": 12345,
    "advanced": false,
    "memoryInit": "pattern"
  },
  "program": { "source": "R 0x000\nR 0x080\nR 0x000\nR 0x080", "format": "trace" },
  "steps": [
    {
      "kind": "predict",
      "text": "The trace loads `0x000`, then `0x080`, then `0x000` and `0x080` again. Both addresses have index `000`, so both use set 0, which has room for one line. How many of the four loads will hit?",
      "options": ["None", "Two", "All four"],
      "answer": 0,
      "explain": "None. The first two loads bring their lines in for the first time; after that, each load pushes out the line the next one needs.",
      "hint": "Set 0 holds one line. What happens to `0x000` when `0x080` arrives?",
      "callout": { "set": 0 }
    },
    {
      "kind": "instruct",
      "text": "Run the trace: press Run, or press Shift+Space four times.",
      "expect": {
        "all": [
          { "counter": "accesses", "op": "==", "value": 4 },
          { "counter": "misses", "op": "==", "value": 4 },
          { "counter": "compulsory", "op": "==", "value": 2 },
          { "counter": "conflict", "op": "==", "value": 2 }
        ]
      },
      "hint": "Press Run in the transport bar. If the trace has already run, press Reset, then Run.",
      "showMe": [{ "op": "run", "n": 4 }],
      "callout": "transport"
    },
    {
      "kind": "explain",
      "text": "The stats show four misses. The first two are *compulsory*: those lines had never been loaded. The other two are *conflict* misses: the line had been there, but another line in the same set pushed it out while the other seven sets stood empty. A cache that could put any line anywhere would have kept it.",
      "callout": { "set": 0 }
    },
    {
      "kind": "instruct",
      "text": "Give the cache two ways: set Associativity to 2-way. The cache will ask to reset; choose Reset and apply.",
      "expect": { "config": { "associativity": 2 } },
      "hint": "Associativity is in the bar above the cache. Pick the 2-way option.",
      "showMe": [
        {
          "op": "config",
          "config": {
            "addressBits": 12,
            "wordBytes": 4,
            "cache": {
              "sizeBytes": 128,
              "lineBytes": 16,
              "associativity": 2,
              "replacement": "lru",
              "writePolicy": "writeBack",
              "allocatePolicy": "writeAllocate"
            },
            "seed": 12345,
            "advanced": false,
            "memoryInit": "pattern"
          }
        }
      ],
      "callout": "config"
    },
    {
      "kind": "instruct",
      "text": "Run the same trace again.",
      "expect": {
        "all": [
          { "counter": "accesses", "op": "==", "value": 4 },
          { "counter": "hits", "op": "==", "value": 2 },
          { "counter": "conflict", "op": "==", "value": 0 }
        ]
      },
      "hint": "Press Run in the transport bar.",
      "showMe": [{ "op": "run", "n": 4 }],
      "callout": "transport"
    },
    {
      "kind": "explain",
      "text": "Two misses, both compulsory, and two hits. With two ways, set 0 holds `0x000` and `0x080` together, so neither pushes the other out.",
      "callout": { "set": 0 }
    }
  ]
}
```

Reading it through:

- **Preset.** A 128-byte cache of 16-byte lines, direct-mapped, with the memory pattern turned on so every word has a recognisable value. It is basic (write-back, write-allocate, untimed), so `advanced` is `false` in both places.
- **Program.** Four loads that alternate between two addresses sharing set 0. The loads are a trace, so Run and Shift+Space can play them.
- **Step 1 (predict).** The student commits to a count before seeing it. `callout: { "set": 0 }` outlines the set the answer depends on. There is no `then`, so the answer reveals nothing on the workbench; the next step runs the trace.
- **Step 2 (instruct).** `expect` is an `all` of four counter checks: all four accesses missed, two compulsory and two conflict. `showMe` is `{ "op": "run", "n": 4 }`. The `hint` is shown under the text.
- **Step 4 (instruct, config).** `expect` is a `config` predicate on `associativity`. Its `showMe` is a `config` op holding the whole new configuration, which differs from the preset in `associativity` alone. That difference is what the `config` predicate checks, so the validator accepts the step.
- **Step 5 (instruct).** The same trace again, now under two ways. Later steps are validated against the 2-way configuration.
- **Callouts.** `"transport"` and `"config"` name regions; `{ "set": 0 }` names a cache set.

For the other kinds, read: `modules/07-write-back-stale-memory.json` for `observe`, an `edit` op, `event` and `lastAccess` predicates and a `then` list; `modules/05-fifo-vs-lru.json` for a configuration change in the middle of a module; `modules/09-amat.json` for timed mode and the `cycles` counter.

## Adding a module

1. **Write the file.** Make `_instruments/cache-explorer/modules/NN-short-name.json`. The number is the next free one; the name is lower case with hyphens; the `id` field is the name without `.json`.
2. **Add its row to `modules/index.json`.** The index is an array, in picker order, and each row repeats four of the file's fields:

   ```json
   {
     "id": "10-short-name",
     "title": "The same title as the file",
     "description": "The same description as the file.",
     "advanced": false
   }
   ```

   The core's test reads both and fails if a row disagrees with its file, if an id is listed twice, or if a file has no row or a row no file.
3. **Add it to the core's module tests**, `_instruments/crates/cache-explorer-core/tests/modules.rs`:
   - an `include_str!` constant beside the others (`const MODULE_10: &str = include_str!("../../../cache-explorer/modules/10-short-name.json");`);
   - the constant in the `SHIPPED` array at its index position (the test zips `SHIPPED` with the index rows), and the array's length;
   - a line in the `cases!` list that runs the module through its Show me path: `shipped_module_10_completes_through_its_show_me_path => (MODULE_10),`.

   A shipped module therefore needs a `showMe` that completes every instruct step, and every observe `until` must be reachable in the run. (`showMe` is optional for the format, not for a shipped module.)

   The Show me path test starts the module, applies every step's Show me or answer, and fails unless the module reaches the done screen. It is what proves that every `expect` and `until` can be made true by its own ops. A module that cannot complete this way is broken for a student who gets stuck.
4. **Run the tests** from `_instruments/`:

   ```sh
   cargo test -p cache-explorer-core --test modules --locked
   ```

   Parse errors come with the path of the field. The web tests and the development page read the `modules/` folder as it is, so a new module shows in the picker with no further change.
5. **Look at it.** Run `npm run dev` in `web/` and open the page; pick the module and work through it by hand, once with Show me and once without. Check each callout outlines what the step is about.
6. **Rebuild the committed assets.** The build copies `modules/*.json` into `assets/cache-explorer/modules/`, and those copies are committed. See the README.
