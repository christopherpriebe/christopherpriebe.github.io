/* tslint:disable */
/* eslint-disable */
/**
 * A button of the module panel, and whether it is live.
 */
export interface ModuleAction {
    /**
     * The button.
     */
    action: ModuleActionKind;
    /**
     * Whether it is live; a button shown disabled is not.
     */
    enabled: boolean;
}

/**
 * A button of the module panel.
 */
export type ModuleActionKind = "check" | "next" | "showMe" | "restart";

/**
 * A byte address, below `2^address_bits` of the configuration in force.
 */
export type Addr = number;

/**
 * A cache and machine configuration. Fixed for a run; a new configuration
 * starts a new run.
 */
export interface Config {
    /**
     * Width of a byte address, from [`MIN_ADDRESS_BITS`] to
     * [`MAX_ADDRESS_BITS`]. `Config::default()` uses 12.
     */
    addressBits: number;
    /**
     * Bytes per word.
     */
    wordBytes: WordBytes;
    /**
     * Cache organisation and policies.
     */
    cache: CacheConfig;
    /**
     * `Some` turns on timed mode.
     */
    timing?: Timing;
    /**
     * Seed for random replacement; at most [`MAX_SEED`], 2^53 − 1, so it
     * survives a JavaScript number.
     */
    seed: number;
    /**
     * Presentation flag carried for share links: whether the student chose
     * Advanced. The run never reads it, so two configurations that differ
     * only here produce identical runs.
     */
    advanced?: boolean;
    /**
     * Contents of memory no run has written yet.
     */
    memoryInit?: MemoryInit;
}

/**
 * A cell or counter an event changed: one thing the page can highlight.
 *
 * Sorted by variant, then by field, so a list of them has one order
 * whatever produced it.
 */
export type ChangeRef = { cell: "valid"; set: SetIndex; way: WayIndex } | { cell: "tag"; set: SetIndex; way: WayIndex } | { cell: "dirty"; set: SetIndex; way: WayIndex } | { cell: "rank"; set: SetIndex; way: WayIndex } | { cell: "word"; set: SetIndex; way: WayIndex; word: WordIndex } | { cell: "memory"; addr: Addr } | { cell: "register"; register: Register } | { cell: "pc" } | { cell: "counter"; counter: Counter };

/**
 * A condition on the machine or the log at a position of the session.
 *
 * Serialized keyed by which field it has: a way predicate as
 * `{"way":{"set":2,"way":0},"valid":true}`, a counter predicate as
 * `{"counter":"misses","op":">=","value":2}`. Read through a flat form with
 * every key optional, as an op is, and nested at most
 * [`MAX_PREDICATE_DEPTH`] deep.
 */
export type Predicate = WayPredicate | LinePredicate | MemoryPredicate | CounterPredicate | RegisterPredicate | AllPredicate | AnyPredicate | NotPredicate | EventPredicate | LastAccessPredicate | ConfigPredicate;

/**
 * A configuration checked on its own, for module authoring tools and
 * tests: its summary and geometry, or why it is invalid.
 */
export type ConfigCheck = { status: "valid"; summary: string; geometry: Geometry } | { status: "invalid"; error: ConfigErrorView };

/**
 * A control of the page that the snapshot can switch off.
 */
export type ControlAction = "undo" | "redo" | "step" | "stepAccess" | "run" | "reset" | "issue" | "edit" | "configure" | "program" | "load";

/**
 * A cycle count within a run.
 */
export type Cycle = number;

/**
 * A data value, zero-extended from the configuration's word size.
 *
 * It serializes as a 128-bit integer: `serde_json` writes that as a plain
 * number, exact to 2^64 − 1, and serde-wasm-bindgen hands it to JavaScript
 * as a `BigInt`, since a JavaScript number cannot hold every 8-byte word. It
 * deserializes from any unsigned 64-bit integer, so JSON numbers, JavaScript
 * numbers that are safe integers, and `BigInt`s are all accepted.
 */
export type Word = bigint;

/**
 * A field of a configuration, as an error names the one to change. No rule
 * names the word size: every word size is valid on its own, and a line too
 * short for its word is repaired by the line.
 */
export type ConfigField = "addressBits" | "wordBytes" | "sizeBytes" | "lineBytes" | "associativity" | "timing" | "seed";

/**
 * A figure the Stats region shows: its exact value, and the text the
 * student reads, which is rounded.
 */
export interface Figure {
    /**
     * The exact value, unrounded.
     */
    value: number;
    /**
     * The text, as in "AMAT = 1 + 0.40 × 20 = 9.0 cycles".
     */
    text: string;
}

/**
 * A group of the configuration bar.
 */
export type ConfigGroupKind = "cacheSize" | "lineSize" | "associativity" | "replacement" | "addressBits" | "wordSize" | "writePolicy" | "allocatePolicy";

/**
 * A guided module, read and checked by [`Module::parse`]. Serialized in
 * the module format, addresses as numbers.
 */
export interface Module {
    /**
     * `04-tracking-lru`: two digits, then a name.
     */
    id: string;
    /**
     * "Tracking LRU".
     */
    title: string;
    /**
     * What the module teaches, in the inline markdown steps use.
     */
    description: string;
    /**
     * Whether the module needs Advanced.
     */
    advanced: boolean;
    /**
     * The configuration the module starts with.
     */
    preset: Config;
    /**
     * The program or trace the module loads when it starts.
     */
    program?: ProgramSource;
    /**
     * The steps, at least one.
     */
    steps: Step[];
}

/**
 * A plan or a refusal, tagged under `outcome` so that TypeScript can tell
 * them apart.
 */
export type EditOutcome = ({ outcome: "plan" } & EditPlan) | ({ outcome: "refusal" } & EditRefusal);

/**
 * A position in the session's log of micro-events: 0 is the state before the
 * first event, and k the state after the k-th event.
 */
export type EventPos = number;

/**
 * A register and its value.
 */
export interface RegisterView {
    /**
     * The register.
     */
    register: Register;
    /**
     * Its value: "53".
     */
    text: string;
}

/**
 * A set row of the cache, or with `way` one way cell of it.
 */
export interface CellRef {
    /**
     * The set.
     */
    set: SetIndex;
    /**
     * The way, for a cell rather than the whole row.
     */
    way?: WayIndex;
}

/**
 * A set, below the geometry's number of sets.
 */
export type SetIndex = number;

/**
 * A shared run in a line: the opening run's configuration, its steps and
 * where it opens, for the loading state and for Share's status line. The
 * opening run is the one holding the cursor, or the last.
 */
export interface ShareSummary {
    /**
     * The opening run's configuration: "2-way LRU · 128 B".
     */
    configSummary: string;
    /**
     * The runs in the log.
     */
    runs: number;
    /**
     * The ops in the log.
     */
    opCount: number;
    /**
     * The opening run's steps; while replaying, those replayed so far.
     */
    stepCount: number;
    /**
     * The step the run opens at, 0 before the first; while replaying, the
     * steps so far until the replay has passed it.
     */
    openingStep: number;
    /**
     * "2-way LRU · 128 B · 14 steps, opening at step 9."
     */
    text: string;
    /**
     * "Link copied. It encodes the configuration and all 14 steps, and
     * opens at step 9."
     */
    copiedText: string;
    /**
     * One byte per step of the opening run replayed so far, in order: bit 0
     * (1) is set on an entry's first step and bit 1 (2) on every step of an
     * entry that missed. They are the bytes the session's `ticks()`
     * returns, there as a `Uint8Array` and here as a `number[]`.
     */
    ticks: number[];
}

/**
 * A stale word of memory: the dirty way holding a newer value.
 */
export interface Stale {
    /**
     * The set of the way holding the newer value.
     */
    set: SetIndex;
    /**
     * The way.
     */
    way: WayIndex;
    /**
     * "Stale: set 2, way 1 holds the newer value 7."
     */
    text: string;
}

/**
 * A step's kind, as the panel shows it, or the done screen.
 */
export type ModuleStepKind = "instruct" | "predict" | "observe" | "explain" | "done";

/**
 * A tag, below `2^tag_bits` of the geometry in force.
 */
export type Tag = number;

/**
 * A typed address, read against a geometry.
 */
export type AddressInput = { status: "empty" } | { status: "valid"; decoded: Decoded } | { status: "misaligned"; decoded: Decoded; message: string } | { status: "invalid"; message: string };

/**
 * A typed data value, read against a word size.
 */
export type WordInput = { status: "empty" } | { status: "valid"; value: Word; text: string } | { status: "invalid"; message: string };

/**
 * A typed hit time or miss penalty, applied to a configuration.
 */
export type ConfigNumberInput = { status: "valid"; config: Config } | { status: "invalid"; message: string };

/**
 * A way of the cache, named by its set and its number. A way predicate
 * names its way this way too; one read from JSON is unchecked until the
 * module is checked against the configuration in force.
 */
export interface WayLoc {
    /**
     * The set.
     */
    set: SetIndex;
    /**
     * The way.
     */
    way: WayIndex;
}

/**
 * A way within a set, below the geometry's number of ways.
 */
export type WayIndex = number;

/**
 * A way's rank, and its mark at either end of the order.
 */
export interface RankCell {
    /**
     * The rank.
     */
    value: number;
    /**
     * The mark, for the first and the last rank.
     */
    role?: RankRole;
}

/**
 * A word within a line, below the geometry's words per line.
 */
export type WordIndex = number;

/**
 * An action that opens another document instead.
 */
export interface LoadOption {
    /**
     * The action's label.
     */
    label: string;
    /**
     * The document it opens, as a share string.
     */
    share: string;
}

/**
 * An address split into its tag, index and offset under one geometry, with
 * the texts the address strip and the log show for it.
 */
export interface Decoded {
    /**
     * The address.
     */
    addr: Addr;
    /**
     * The address in hex, padded to the address width: `0x0A4`.
     */
    addrText: string;
    /**
     * The address of the line holding it: the offset bits cleared.
     */
    line: LineAddr;
    /**
     * The line address in hex, padded to the address width: `0x0A0`.
     */
    lineText: string;
    /**
     * The tag.
     */
    tag: Tag;
    /**
     * The tag in hex, padded to the tag's width and to at least two digits:
     * `0x01`.
     */
    tagText: string;
    /**
     * The set the address maps to; 0 when the cache is fully associative.
     */
    index: SetIndex;
    /**
     * The byte within the line.
     */
    offset: number;
    /**
     * The word within the line.
     */
    word: WordIndex;
    /**
     * The byte within the word; 0 for an aligned address.
     */
    byteInWord: number;
    /**
     * Whether the address is a multiple of the word size. A misaligned
     * address decodes, so the strip follows it while the student types, but
     * cannot be issued.
     */
    aligned: boolean;
    /**
     * Where each field lies in the address.
     */
    bitRanges: BitRanges;
    /**
     * The address in binary, one character per address bit, most significant
     * first.
     */
    bits: string;
    /**
     * The non-empty fields, most significant first: tag, index, offset.
     */
    fields: AddressField[];
}

/**
 * An address, as a line or memory predicate names one.
 */
export interface AddrLoc {
    /**
     * The address: a number, or in module JSON a hex string such as
     * `"0x0A4"`.
     */
    addr: Addr;
}

/**
 * An area of the workbench a callout outlines.
 */
export type CalloutArea = "addressStrip" | "cache" | "memory" | "log" | "stats" | "config" | "cpu" | "transport";

/**
 * An edit the rules accept: what it changes and what follows from it, and
 * how the editor presents it.
 */
export interface EditPlan {
    /**
     * The way or word it edits.
     */
    target: EditTarget;
    /**
     * The patch with every field the result has: the line at its start,
     * and the defaults the edit implies filled in. Planned again, it gives
     * the same plan.
     */
    patch: EditPatch;
    /**
     * The status bar's colour.
     */
    tone: Tone;
    /**
     * The status bar's title: "Ready to apply", "Memory only".
     */
    title: string;
    /**
     * The status bar's text.
     */
    text: string;
    /**
     * Every cell it changes, with its text before and after, in cell order.
     */
    changes: FieldChange[];
    /**
     * What follows from it.
     */
    consequences: Consequence[];
    /**
     * What it changes, one line each: "word 1: 53 → 9", "rank way 1: 1 →
     * 0".
     */
    preview: string[];
    /**
     * The primary button: "Apply", "Apply to memory", "Apply to both".
     */
    action: string;
    /**
     * One mark per data input of the way editor; none for a memory edit.
     */
    wordMarks: WordMark[];
}

/**
 * An edit the rules refuse: which rule, why in words, and another edit to
 * offer, if one would do.
 */
export interface EditRefusal {
    /**
     * The rule.
     */
    reason: RefusalReason;
    /**
     * The status bar's colour.
     */
    tone: Tone;
    /**
     * The status bar's title: "Refused", "Data differs from memory".
     */
    title: string;
    /**
     * The status bar's text.
     */
    text: string;
    /**
     * The edit offered instead, boxed to keep a refusal small beside a
     * plan.
     */
    suggestion?: Suggestion;
    /**
     * One mark per data input of the way editor, as the edit would leave
     * them; none for a memory edit or a patch that does not fit its way.
     */
    wordMarks: WordMark[];
}

/**
 * An index into the session's ops.
 */
export type OpIndex = number;

/**
 * Another edit a refusal offers: a complete target and patch the rules
 * accept, the label of the button that applies it, and its preview.
 */
export interface Suggestion {
    /**
     * The way or word it edits.
     */
    target: EditTarget;
    /**
     * What it changes.
     */
    patch: EditPatch;
    /**
     * Its button: "Set dirty and apply", "Move to set 0, way 0".
     */
    action: string;
    /**
     * What it changes, one line each, as a plan's preview.
     */
    preview: string[];
}

/**
 * Bytes per word. Serialized as the number 1, 2, 4 or 8.
 */
export type WordBytes = 1 | 2 | 4 | 8;

/**
 * Contents of memory no run has written yet.
 */
export type MemoryInit = "zero" | "pattern";

/**
 * Entries of the log in order, as the boundary returns a window of them:
 * an array in JavaScript, which one serializer call builds.
 */
export type EntryViews = EntryView[];

/**
 * Events in order, as the boundary returns a list of them: an array in
 * JavaScript, which one serializer call builds.
 */
export type MicroEvents = MicroEvent[];

/**
 * Everything the configuration bar draws for one configuration.
 */
export interface ConfigControls {
    /**
     * The configuration's summary in the mode shown: "128 B · 16 B lines ·
     * 2-way · LRU".
     */
    summary: string;
    /**
     * The groups, in display order.
     */
    groups: ConfigGroup[];
    /**
     * The timing controls.
     */
    timing: TimingControl;
    /**
     * Why the configuration itself is invalid, when it is: one from a
     * malformed link or module.
     */
    error?: ConfigErrorView;
}

/**
 * Everything the memory editor draws for its typed value.
 */
export interface MemoryEditDraft {
    /**
     * The value, read.
     */
    value: FieldInput;
    /**
     * The edit Apply sends; absent while the value does not read.
     */
    patch?: EditPatch;
    /**
     * The patch's plan or refusal.
     */
    outcome: EditOutcome;
}

/**
 * Everything the module panel draws.
 */
export interface ModuleView {
    /**
     * The module's id.
     */
    id: string;
    /**
     * The module's title.
     */
    title: string;
    /**
     * What the module teaches.
     */
    description: TextSegment[];
    /**
     * The current step's index, from 0; the number of steps on the done
     * screen.
     */
    stepIndex: number;
    /**
     * The number of steps.
     */
    stepCount: number;
    /**
     * The current step's kind, or the done screen.
     */
    kind: ModuleStepKind;
    /**
     * The kind's label: "Do this", "Predict", "Watch", "Explain", or
     * "Done".
     */
    kindLabel: string;
    /**
     * "Step 3 of 6", or "6 of 6" on the done screen.
     */
    progressText: string;
    /**
     * The step's text, or the done screen's.
     */
    text: TextSegment[];
    /**
     * An instruct step's hint.
     */
    hint?: TextSegment[];
    /**
     * A predict step's options, with their states.
     */
    options?: OptionView[];
    /**
     * What checking the prediction, or revealing it, found.
     */
    feedback?: Feedback;
    /**
     * The region the step points at.
     */
    callout?: Callout;
    /**
     * Why the last "Show me", Restart or prediction `then` was refused,
     * such as at the session's limit; the step keeps waiting.
     */
    notice?: string;
    /**
     * The panel's buttons, in order; a button not listed is not shown.
     */
    actions: ModuleAction[];
    /**
     * The picker's line while the module runs: "Running 04 · Tracking LRU
     * · step 3 of 6".
     */
    drawerSummary: string;
}

/**
 * Everything the page draws at the cursor.
 */
export interface State {
    /**
     * The displayed run's configuration.
     */
    config: Config;
    /**
     * Whether the page shows Advanced: the mode chosen, or Advanced while
     * the displayed run needs it.
     */
    advanced: boolean;
    /**
     * The displayed run's geometry.
     */
    geometry: Geometry;
    /**
     * Where the cursor is.
     */
    position: Position;
    /**
     * The event the cursor rests on; absent at position 0.
     */
    event?: MicroEvent;
    /**
     * The cache table.
     */
    cache: CacheView;
    /**
     * The memory table's heading and size; its rows come from
     * `memoryLines`.
     */
    memory: MemoryView;
    /**
     * The transfer lane between the cache and memory.
     */
    bus: BusView;
    /**
     * The request lane between the CPU and the cache.
     */
    request: RequestView;
    /**
     * The CPU box.
     */
    cpu: CpuView;
    /**
     * The loaded program, when there is one.
     */
    program?: ProgramView;
    /**
     * The Stats region.
     */
    stats: Stats;
    /**
     * What the cursor's entry has changed so far, net: the cells, words
     * and counters to draw on the changed ground. Empty at a run's origin
     * and on a program load.
     */
    highlight: ChangeRef[];
    /**
     * The highlight in words, as the log's "changed" line gives it:
     * "set 2 way 0 valid, tag, data · r1 · misses", or "nothing changed".
     */
    changedSummary: string;
    /**
     * Which controls are live.
     */
    controls: Controls;
    /**
     * The status line and the configuration's summary.
     */
    labels: Labels;
}

/**
 * Everything the way editor draws for its form: each field read, the
 * swatches under the line address, what the untouched data inputs show,
 * the smallest patch that makes the way what the form shows, its plan or
 * refusal, and the way as it would be.
 */
export interface EditDraft {
    /**
     * The line address, read.
     */
    line: FieldInput;
    /**
     * Each data input, read.
     */
    words: FieldInput[];
    /**
     * The typed line's tag, `0x05`, for its swatch.
     */
    tagText?: string;
    /**
     * The typed line's index, `2`, for its swatch; absent when the cache
     * has no index bits.
     */
    indexText?: string;
    /**
     * What each data input shows while the student has not typed in it:
     * memory's copy of the line when the way takes a line, and otherwise
     * the way's words.
     */
    wordTexts: string[];
    /**
     * The edit Apply sends; absent while a field does not read.
     */
    patch?: EditPatch;
    /**
     * The patch's plan or refusal; a refusal naming the field while one
     * does not read.
     */
    outcome: EditOutcome;
    /**
     * The way as the edit would leave it.
     */
    result: WayView;
}

/**
 * How a cache places lines. A one-line cache is both direct-mapped and fully
 * associative by definition; it is `FullyAssociative`, so it has no index.
 */
export type Placement = "directMapped" | "setAssociative" | "fullyAssociative";

/**
 * How a counter predicate compares.
 */
export type CmpOp = "==" | ">=" | "<=";

/**
 * How a data input of the way editor is marked: underlined when its value
 * differs from the way's word, on a tinted ground when it differs from
 * memory's copy.
 */
export type WordMark = "same" | "changed" | "differsFromMemory" | "changedAndDiffers";

/**
 * How a load is going, after a slice.
 */
export type LoadProgress = { status: "replaying"; summary: ShareSummary } | { status: "done"; summary: ShareSummary } | { status: "failed"; failure: LoadFailure };

/**
 * How a program's source is written.
 */
export type ProgramFormat = "asm" | "trace";

/**
 * How a run of text is set.
 */
export type TextStyle = "plain" | "code" | "emphasis";

/**
 * How an editor's status bar reads, as the colour of its rule: ready to
 * apply (green), a caution (amber), a move to another way (navy), refused
 * (red), or nothing to apply yet (the rule's grey).
 */
export type Tone = "ready" | "caution" | "move" | "refused" | "idle";

/**
 * How an option is drawn.
 */
export type OptionState = "idle" | "selected" | "wrong" | "correct" | "shown";

/**
 * How much ends at a position; each level implies the ones before it.
 */
export type Boundary = "midEntry" | "entryEnd" | "opEnd" | "logEnd";

/**
 * One cell an edit changes, with its text before and after, as the cache
 * table and memory show it: `0` and `1` for a bit, `—` for a field an empty
 * way does not have.
 */
export interface FieldChange {
    /**
     * The cell.
     */
    cell: ChangeRef;
    /**
     * Its text before the edit.
     */
    from: string;
    /**
     * Its text after.
     */
    to: string;
}

/**
 * One entry of the log, as its row shows it.
 */
export interface EntryView {
    /**
     * Its place among the run's entries, from 0.
     */
    index: number;
    /**
     * Its number within the run, from 1; absent for a program load.
     */
    number?: number;
    /**
     * What it is, in a few words: the instruction, as in `ld r3, [0x2A4]`;
     * "edit set 2 way 0"; "Loaded a program of 5 instructions" for a load;
     * for an error, the instruction the program stopped at, or "—".
     */
    label: string;
    /**
     * What it is.
     */
    kind: EntryKind;
    /**
     * What it did.
     */
    outcome: EntryOutcome;
    /**
     * What it did, in words: "miss · evict · write-back", "hit", "edit",
     * "CPU only", "program", "error".
     */
    outcomeText: string;
    /**
     * The position of its first event.
     */
    first: EventPos;
    /**
     * The position of its last event.
     */
    last: EventPos;
    /**
     * Where it is, relative to the cursor.
     */
    tense: Tense;
    /**
     * What it changed: over the whole entry for a past entry, up to the
     * cursor for the current one; absent for a future entry and for a
     * program load.
     */
    changedSummary?: string;
    /**
     * The current entry's events, in order; empty for every other entry.
     */
    events: EventRow[];
}

/**
 * One error in a program or trace, and where it is.
 */
export interface ParseError {
    /**
     * The source line, from 1.
     */
    line: number;
    /**
     * The column where the problem starts, from 1, counted in Unicode
     * scalar values.
     */
    column: number;
    /**
     * How many characters to underline from there; 0 when there is nothing
     * to underline, as for an empty source or an empty line past the line
     * limit.
     */
    length: number;
    /**
     * What is wrong: ``unknown instruction `lw`; use ld, st, add or bnez.``
     */
    message: string;
    /**
     * The message with its place:
     * ``Line 3, column 5: unknown instruction `lw`; use ld, st, add or bnez.``
     */
    text: string;
}

/**
 * One event of the current entry, as its row in the log shows it.
 */
export interface EventRow {
    /**
     * The position the cursor rests on at this event.
     */
    pos: EventPos;
    /**
     * Its label, `5.3`; empty for a program load.
     */
    label: string;
    /**
     * What happened.
     */
    kind: EventKind;
    /**
     * Its log line.
     */
    text: string;
    /**
     * The configuration group responsible, for the row's "why:" link.
     */
    cause?: Cause;
    /**
     * Where it is, relative to the cursor.
     */
    tense: Tense;
}

/**
 * One field of a decoded address as the address strip shows it.
 */
export interface AddressField {
    /**
     * Which field this is.
     */
    field: FieldKind;
    /**
     * The field's bits, most significant first, one character per bit.
     */
    bits: string;
    /**
     * The field's value as the strip labels it: the tag in hex (`0x01`), the
     * index and the offset in decimal (`2`, `4`).
     */
    valueText: string;
    /**
     * The note under the field: `5 bits`, `3 bits → set 2`,
     * `4 bits → word 1, byte 0`.
     */
    note: string;
    /**
     * Where the field lies in the address.
     */
    range: BitRange;
}

/**
 * One group of the configuration bar and its options.
 */
export interface ConfigGroup {
    /**
     * Which group.
     */
    group: ConfigGroupKind;
    /**
     * Whether Basics hides it.
     */
    advancedOnly: boolean;
    /**
     * Its options, in display order.
     */
    options: ConfigOption[];
}

/**
 * One item in the session log. The session's state is a fold over these.
 *
 * Serialized with its kind under `op`: `{"op":"config","config":{…}}`,
 * `{"op":"issue","kind":"write","addr":164,"value":7}`,
 * `{"op":"program","source":"…","format":"asm"}`, `{"op":"step"}`,
 * `{"op":"stepAccess"}`, `{"op":"run","n":20}` and
 * `{"op":"edit","target":{…},"patch":{…}}`. It is read through a flat form
 * with every field optional, which then requires the fields of its kind and
 * refuses any other: that keeps serde's line and column in the errors of a
 * share document and avoids the code serde generates to buffer an
 * internally tagged enum.
 */
export type Op = { op: "config"; config: Config } | { op: "issue"; kind: AccessKind; addr: Addr; value?: Word } | { op: "program"; source: string; format: ProgramFormat } | { op: "step" } | { op: "stepAccess" } | { op: "run"; n: number } | { op: "edit"; target: EditTarget; patch: EditPatch };

/**
 * One line of memory, as its row shows it.
 */
export interface MemoryLineView {
    /**
     * The line's address.
     */
    line: LineAddr;
    /**
     * The address: `0x0A0`.
     */
    lineText: string;
    /**
     * The line's tag: `0x02`.
     */
    tagText: string;
    /**
     * The set it maps to.
     */
    set: SetIndex;
    /**
     * The way holding it, when the cache does.
     */
    resident?: WayLoc;
    /**
     * Whether it is the line the event at the cursor moves.
     */
    onBus: boolean;
    /**
     * Whether the student asked to watch it: a "Jump to" row.
     */
    watched: boolean;
    /**
     * Its words, in order.
     */
    words: MemoryWordView[];
}

/**
 * One line of the loaded program's source.
 */
export interface ProgramLine {
    /**
     * Its number, from 1.
     */
    number: number;
    /**
     * The line as written.
     */
    text: string;
}

/**
 * One module as the picker lists it.
 */
export interface ModuleCard {
    /**
     * The module's id, which names its file.
     */
    id: string;
    /**
     * Its number, the id's two digits: "04".
     */
    number: string;
    /**
     * Its title.
     */
    title: string;
    /**
     * What it teaches, as segments.
     */
    description: TextSegment[];
    /**
     * Whether it needs Advanced, which the card marks.
     */
    advanced: boolean;
}

/**
 * One of the eight 64-bit registers. `r0` reads 0, and writes to it are
 * dropped.
 */
export type Register = "r0" | "r1" | "r2" | "r3" | "r4" | "r5" | "r6" | "r7";

/**
 * One of the run's counters.
 */
export type Counter = "accesses" | "loads" | "stores" | "hits" | "misses" | "evictions" | "writebacks" | "writethroughs" | "writearounds" | "fills" | "cycles" | "compulsory" | "capacity" | "conflict";

/**
 * One of the two timing numbers: the hit time or the miss penalty.
 */
export type ConfigNumberField = "hitCycles" | "missCycles";

/**
 * One option of a prediction.
 */
export interface OptionView {
    /**
     * Its text.
     */
    text: TextSegment[];
    /**
     * How it is drawn.
     */
    state: OptionState;
}

/**
 * One option of the configuration bar, with the whole configuration a
 * click on it produces.
 */
export interface ConfigOption {
    /**
     * Its label: "Direct", "2-way", "Full", "64 B", "LRU", "Write-back",
     * "No allocate".
     */
    label: string;
    /**
     * Whether it is the configuration's current choice.
     */
    pressed: boolean;
    /**
     * Whether it cannot be chosen.
     */
    disabled: boolean;
    /**
     * Why it cannot be chosen: the message of the rule its configuration
     * breaks, or "Same as Full for an 8-line cache."
     */
    title?: string;
    /**
     * The configuration a click produces, for `proposeConfig` as it is.
     */
    config: Config;
}

/**
 * One row of the Stats region: which it is, its value as text, and whether
 * to show it as changed.
 */
export interface StatRow {
    /**
     * Which row it is.
     */
    key: StatKey;
    /**
     * The value: a count such as "12" or "10,000", or the hit rate such as
     * "67%", which is an em dash before the run's first access.
     */
    text: string;
    /**
     * True when a counter the row shows has moved since the cursor's entry
     * began, so the row takes the changed ground as the cache's cells do.
     */
    changed: boolean;
}

/**
 * One run of text in one style.
 */
export interface TextSegment {
    /**
     * The text, as the page shows it.
     */
    text: string;
    /**
     * How the page sets it.
     */
    style: TextStyle;
}

/**
 * One set of the cache.
 */
export interface SetView {
    /**
     * The set.
     */
    set: SetIndex;
    /**
     * Its ways, in order.
     */
    ways: WayView[];
}

/**
 * One step of a module. Serialized with its kind under `kind`.
 *
 * Every step has text, and may point at one region of the workbench with a
 * callout. Read through a flat form with every field optional, as an op
 * is.
 */
export type Step = { kind: "instruct"; text: string; expect: Predicate; hint?: string; showMe?: Op[]; callout?: Callout } | { kind: "predict"; text: string; options: string[]; answer: number; explain: string; then?: Op[]; hint?: string; callout?: Callout } | { kind: "observe"; text: string; until: Predicate; callout?: Callout } | { kind: "explain"; text: string; callout?: Callout };

/**
 * One step of an op, as the log shows it.
 */
export interface MicroEvent {
    /**
     * The event's place in the whole log, from 0: the event after which the
     * cursor rests at position k has id k − 1. Ids are reused after the log
     * is truncated, so an id is never a key that outlives a truncation.
     */
    id: number;
    /**
     * The op that produced the event.
     */
    opIndex: OpIndex;
    /**
     * The run the event belongs to, from 1.
     */
    run: number;
    /**
     * The entry within the run, from 1; absent for a setup.
     */
    entry?: number;
    /**
     * The event's number within its entry, from 1.
     */
    seq: number;
    /**
     * `{entry}.{seq}`, such as `5.3`; empty for a setup.
     */
    label: string;
    /**
     * The run's cycle count once the event's effect is complete; present in
     * timed mode only.
     */
    cycle?: Cycle;
    /**
     * What happened.
     */
    kind: EventKind;
    /**
     * The address the event concerns: the accessed address, or the line
     * moving to or from memory.
     */
    addr?: Addr;
    /**
     * The set the event concerns.
     */
    set?: SetIndex;
    /**
     * The way the event concerns.
     */
    way?: WayIndex;
    /**
     * Every cell and counter whose value this event changed, sorted and
     * without duplicates.
     */
    changed: ChangeRef[];
    /**
     * The log line.
     */
    explain: string;
    /**
     * The configuration group responsible for the event, which the log's
     * "why:" link flashes.
     */
    cause?: Cause;
}

/**
 * One typed field of an editor, read.
 */
export type FieldInput = { status: "empty" } | { status: "valid"; text: string } | { status: "invalid"; message: string };

/**
 * One way of the cache, as its cell shows it.
 */
export interface WayView {
    /**
     * Whether it holds a line.
     */
    valid: boolean;
    /**
     * Its tag, `0x05`; absent while invalid, which the cell shows as `—`.
     */
    tagText?: string;
    /**
     * The address of its line, `0x0A0`; absent while invalid.
     */
    lineText?: string;
    /**
     * Whether its line is dirty.
     */
    dirty: boolean;
    /**
     * Its words, in order; empty while invalid, which the cell shows as a
     * `·` per word.
     */
    words: string[];
    /**
     * Its rank, when the rank column is shown.
     */
    rank?: RankCell;
    /**
     * Whether a miss has chosen it and has not filled it yet.
     */
    victim: boolean;
    /**
     * The word the access at the cursor reads or writes, once the way holds
     * the access's line.
     */
    addressedWord?: WordIndex;
    /**
     * What the cell describes itself as: "Set 2, way 0: valid, tag 0x05,
     * dirty, rank 1, victim."
     */
    description: string;
}

/**
 * One word of memory, as its cell shows it.
 */
export interface MemoryWordView {
    /**
     * The word's address.
     */
    addr: Addr;
    /**
     * Its value: "53".
     */
    text: string;
    /**
     * Its stale mark, when a dirty way holds a newer value.
     */
    stale?: Stale;
    /**
     * What the cell describes itself as: "0x0A4 = 53."
     */
    description: string;
}

/**
 * Rows of the memory table in order, as the boundary returns a window of
 * them: an array in JavaScript, which one serializer call builds.
 */
export type MemoryLines = MemoryLineView[];

/**
 * Something that follows from an edit beyond the fields it names.
 */
export type Consequence = { consequence: "rerank"; set: SetIndex; way: WayIndex; from: number; to: number } | { consequence: "loadsLine"; line: LineAddr } | { consequence: "memoryWrite"; addr: Addr; from: Word; to: Word } | { consequence: "cachedCopyUpdated"; set: SetIndex; way: WayIndex; word: WordIndex; from: Word; to: Word } | { consequence: "becomesStale"; addr: Addr } | { consequence: "staysStale"; addr: Addr; set: SetIndex; way: WayIndex } | { consequence: "noLongerStale"; addr: Addr } | { consequence: "discardsDirtyData"; set: SetIndex; way: WayIndex; addrs: Addr[] };

/**
 * The CPU box's hit or miss mark for the access at the cursor.
 */
export type HitMarker = "hit" | "miss" | "none";

/**
 * The CPU box.
 */
export interface CpuView {
    /**
     * The instruction of the cursor's entry: `ld r1, [0x0A4]`; "—" when
     * there is none.
     */
    instruction: string;
    /**
     * What the CPU is doing.
     */
    status: CpuStatus;
    /**
     * The status in words: "Access #5 · step 3 of 8", "Stalled: 40
     * cycles", "Ready", "Finished", "Stopped at line 4".
     */
    statusText: string;
    /**
     * The hit or miss mark.
     */
    hitMarker: HitMarker;
    /**
     * The registers, `r0` to `r7`.
     */
    registers: RegisterView[];
}

/**
 * The access a `lastAccess` predicate wants.
 */
export interface AccessMatch {
    /**
     * Whether it loads or stores.
     */
    kind?: AccessKind;
    /**
     * The word's address: a number, or in module JSON a hex string such as
     * `"0x0A4"`.
     */
    addr?: Addr;
    /**
     * Whether it hit or missed.
     */
    outcome?: Outcome;
}

/**
 * The address of a line: an [`Addr`] with its offset bits cleared.
 */
export type LineAddr = number;

/**
 * The bits of one address field, inclusive at both ends: bit `hi` down to
 * bit `lo`, `width` bits in all. An empty field has no range.
 */
export interface BitRange {
    /**
     * The most significant bit of the field.
     */
    hi: number;
    /**
     * The least significant bit of the field.
     */
    lo: number;
    /**
     * Bits in the field: `hi − lo + 1`.
     */
    width: number;
}

/**
 * The build: the crates' version, and the newest share format it reads and
 * the one it writes.
 */
export interface BuildInfo {
    /**
     * The crates' version, such as `0.1.0`.
     */
    version: string;
    /**
     * The share format: [`SCHEMA_VERSION`].
     */
    schemaVersion: number;
}

/**
 * The cache table.
 */
export interface CacheView {
    /**
     * The heading: "L1 cache · 4 sets × 2 ways × 16 B".
     */
    header: string;
    /**
     * The heads of the word columns: `+0`, `+4`, `+8`, `+C`.
     */
    wordHeads: string[];
    /**
     * Whether the rank column is shown: under LRU and FIFO with more than
     * one way.
     */
    showRank: boolean;
    /**
     * The rank column's head, when it is shown.
     */
    rankHeader?: RankHeader;
    /**
     * Whether the dirty column is shown: always, in Basics as in Advanced
     * and under either write policy, since the mode changes only which
     * controls the page shows.
     */
    showDirty: boolean;
    /**
     * Whether a set's ways wrap onto rows, and the comparators become a
     * badge in each way: with more than four ways.
     */
    wrap: boolean;
    /**
     * The set of the access at the cursor.
     */
    selectedSet?: SetIndex;
    /**
     * One comparator per way of the selected set.
     */
    compare?: WayCompare[];
    /**
     * Every set, in order.
     */
    sets: SetView[];
}

/**
 * The cache's organisation and policies.
 */
export interface CacheConfig {
    /**
     * Total data bytes; a power of two.
     */
    sizeBytes: number;
    /**
     * Bytes per line; a power of two, at least one word.
     */
    lineBytes: number;
    /**
     * Ways per set. 1 is direct-mapped; equal to the line count is fully
     * associative.
     */
    associativity: number;
    /**
     * Which way a miss evicts from a full set.
     */
    replacement: Replacement;
    /**
     * What a store that hits does to memory.
     */
    writePolicy: WritePolicy;
    /**
     * Whether a store that misses brings its line in.
     */
    allocatePolicy: AllocatePolicy;
}

/**
 * The change an edit makes, of the target's kind.
 *
 * Serialized with its kind under `kind`: a way's patch with the fields it
 * gives beside it, `{"kind":"way","line":192,"dirty":true}`, or a memory
 * word's new value, `{"kind":"memory","value":12}`. Read through a flat
 * form, as an [`Op`] is.
 */
export type EditPatch = ({ kind: "way" } & WayPatch) | { kind: "memory"; value: Word };

/**
 * The colour of a prediction's feedback.
 */
export type FeedbackTone = "correct" | "incorrect" | "shown";

/**
 * The configuration bar's timing controls.
 */
export interface TimingControl {
    /**
     * Whether timed mode is on.
     */
    on: boolean;
    /**
     * The hit time's field: the configuration's, or while timed mode is
     * off the most recent in the log, or 1.
     */
    hitText: string;
    /**
     * The miss penalty's field, likewise, or 20.
     */
    missText: string;
    /**
     * The configuration the checkbox produces.
     */
    toggle: Config;
}

/**
 * The configuration group responsible for an event, which the log's "why:"
 * link flashes.
 */
export type Cause = "lineSize" | "associativity" | "replacement" | "writePolicy" | "allocatePolicy" | "timing";

/**
 * The costs of timed mode, in cycles.
 */
export interface Timing {
    /**
     * Cycles every access costs: the hit time.
     */
    hitCycles: number;
    /**
     * Cycles a miss adds, and a dirty write-back adds again: the miss penalty.
     */
    missCycles: number;
}

/**
 * The event an event predicate wants.
 */
export interface EventMatch {
    /**
     * Its kind.
     */
    kind: EventKind;
    /**
     * Its entry's number within the run, from 1.
     */
    entry?: number;
}

/**
 * The fields of a configuration a `config` predicate wants.
 */
export interface ConfigMatch {
    /**
     * The cache's size, in bytes.
     */
    sizeBytes?: number;
    /**
     * The line size, in bytes.
     */
    lineBytes?: number;
    /**
     * The ways per set.
     */
    associativity?: number;
    /**
     * The replacement policy.
     */
    replacement?: Replacement;
    /**
     * The write policy.
     */
    writePolicy?: WritePolicy;
    /**
     * The allocate policy.
     */
    allocatePolicy?: AllocatePolicy;
    /**
     * Whether timed mode is on.
     */
    timed?: boolean;
}

/**
 * The fields of a way an edit changes; a field left out keeps its value,
 * or takes the one the edit implies. A way is given the line it holds by
 * an address, from which the tag follows: a bare tag has no index bits, so
 * it could not be checked against the way's set.
 */
export interface WayPatch {
    /**
     * Whether the way holds a line.
     */
    valid?: boolean;
    /**
     * Any byte address inside the line the way should hold. Its index bits
     * must equal the way's set.
     */
    line?: Addr;
    /**
     * The whole line, one value per word.
     */
    data?: Word[];
    /**
     * Whether the line is dirty.
     */
    dirty?: boolean;
    /**
     * The way's place in its set's replacement order.
     */
    rank?: number;
}

/**
 * The head of the cache table's rank column.
 */
export type RankHeader = "lru" | "age";

/**
 * The loaded program, as the drawer lists it.
 */
export interface ProgramView {
    /**
     * Whether it is assembly or a trace.
     */
    format: ProgramFormat;
    /**
     * Its source as written.
     */
    source: string;
    /**
     * Its source, line by line.
     */
    lines: ProgramLine[];
    /**
     * The line of the instruction at the pc: the next to execute, the one
     * in flight, or the one the program stopped at; absent once it has
     * finished.
     */
    currentLine?: number;
    /**
     * Whether that instruction is in flight: an access that has not had
     * its last event.
     */
    executing: boolean;
    /**
     * How many instructions it has; for a trace, how many accesses.
     */
    instructions: number;
    /**
     * How many it has executed since it was loaded.
     */
    executed: number;
    /**
     * Whether it has executed its last instruction.
     */
    finished: boolean;
    /**
     * The drawer's line: `5 instructions · next: ld r2, [0x0A8]`.
     */
    summary: string;
}

/**
 * The log of the displayed run: which run it is, where it starts and ends,
 * the step the cursor is on, the log's header, the live region's sentence,
 * and the cursor's entry with its events.
 */
export interface Timeline {
    /**
     * The displayed run, from 1.
     */
    run: number;
    /**
     * The runs in the log, the undone ones after the cursor included.
     */
    runs: number;
    /**
     * The run's origin: position 0, or its `Setup` event.
     */
    origin: EventPos;
    /**
     * The position after the run's last event.
     */
    end: EventPos;
    /**
     * The cursor's position.
     */
    cursor: EventPos;
    /**
     * The steps of the run up to the cursor.
     */
    step: number;
    /**
     * The steps of the run.
     */
    stepCount: number;
    /**
     * The log's header: "step 5 of 14", or "no steps yet".
     */
    headerText: string;
    /**
     * What the page's live region reads: the log line at the cursor, after
     * a move across more than one event opened by the entry's outcome, as
     * in `#5 ld r3, [0x2A4]: miss · evict · write-back. Read word 1 of way 1
     * (68) into r3.` Empty at position 0.
     */
    liveText: string;
    /**
     * The run's entries.
     */
    entryCount: number;
    /**
     * The cursor's entry, with its events; absent at a run's origin.
     */
    current?: EntryView;
}

/**
 * The mark under a rank at either end of its set's order.
 */
export type RankRole = "mru" | "lru" | "new" | "old";

/**
 * The memory table's heading and size.
 */
export interface MemoryView {
    /**
     * "5 lines touched".
     */
    caption: string;
    /**
     * The heads of the word columns, as the cache's.
     */
    wordHeads: string[];
    /**
     * The rows: the lines touched and the lines watched.
     */
    lineCount: number;
    /**
     * The row of the line the event at the cursor moves, to scroll it into
     * view.
     */
    busRow?: number;
}

/**
 * The nearest valid configuration to an invalid one, and the label of the
 * action that opens it.
 */
export interface Repair {
    /**
     * The repaired configuration; it validates.
     */
    config: Config;
    /**
     * The action's label, such as "Open with 2-way instead", naming the
     * repair of the first rule that failed.
     */
    label: string;
}

/**
 * The region of the workbench a step points at: an area, or one set row
 * or way cell of the cache. Serialized as the area's name, `"cache"`, or
 * as the cell, `{"set":2,"way":0}`.
 */
export type Callout = CalloutArea | CellRef;

/**
 * The request lane between the CPU and the cache, and where the address
 * strip's address comes from.
 */
export interface RequestView {
    /**
     * Whether an access is in the cache.
     */
    status: RequestStatus;
    /**
     * Whether it loads or stores.
     */
    kind?: AccessKind;
    /**
     * The lane's label: "Load 0x0A4 → cache", or "No request".
     */
    text: string;
    /**
     * The instruction the access stands for: `ld r1, [0x0A4]`.
     */
    source?: string;
    /**
     * The address strip's footer while it shows the access: "from access
     * #5"; empty otherwise, when the page shows its own copy.
     */
    sourceText: string;
    /**
     * The access's address, decoded for the address strip.
     */
    decoded?: Decoded;
}

/**
 * The result of parsing a program or trace, as the program drawer shows it.
 */
export type ParseResult = { status: "ok"; instructions: number; summary: string } | { status: "invalid"; errors: ParseError[] };

/**
 * The shape of a valid configuration, as `Config::validate` computes it.
 *
 * Only `Config::validate` builds a `Geometry`, so its counts are non-zero
 * powers of two and its widths add up to `address_bits` with at least one tag
 * bit; the core divides and shifts by them on that basis. Its fields are
 * private to the core and read through the methods of the same names, so code
 * outside the core can neither build a `Geometry` nor change one:
 *
 * ```compile_fail,E0616
 * let mut geometry = cache_explorer_core::Config::default().validate().unwrap();
 * geometry.address_bits = 200;
 * ```
 */
export interface Geometry {
    /**
     * Width of a byte address.
     */
    addressBits: number;
    /**
     * Bytes per word.
     */
    wordBytes: number;
    /**
     * Bytes per line.
     */
    lineBytes: number;
    /**
     * Ways per set: the associativity.
     */
    ways: number;
    /**
     * Sets in the cache.
     */
    sets: number;
    /**
     * Lines in the cache: `sets × ways`.
     */
    lines: number;
    /**
     * Words per line.
     */
    wordsPerLine: number;
    /**
     * Width of the offset field: `log2(line_bytes)`.
     */
    offsetBits: number;
    /**
     * Width of the index field: `log2(sets)`, 0 when fully associative.
     */
    indexBits: number;
    /**
     * Width of the tag field: the address bits left over; at least 1.
     */
    tagBits: number;
    /**
     * How lines are placed.
     */
    placement: Placement;
}

/**
 * The text of the program drawer and its format, which Space and
 * Shift+Space pass so the session can tell whether the drawer holds a
 * program other than the one loaded.
 */
export interface ProgramSource {
    /**
     * The source as the student wrote it.
     */
    source: string;
    /**
     * Whether the source is assembly or a trace.
     */
    format: ProgramFormat;
}

/**
 * The texts over the workbench.
 */
export interface Labels {
    /**
     * The status line: "Basics · 14 steps in this run", "Advanced · 14
     * steps in this run · timed".
     */
    statusText: string;
    /**
     * The configuration's summary in the mode shown: "128 B · 16 B lines ·
     * 2-way · LRU".
     */
    configSummary: string;
}

/**
 * The traffic between the cache and memory that the counters add up to:
 * the lines and words moved, and their bytes.
 */
export interface Traffic {
    /**
     * Lines read from memory: one per fill.
     */
    lineReads: number;
    /**
     * Lines written to memory: one per write-back.
     */
    lineWrites: number;
    /**
     * Single words written to memory: one per write-through and one per
     * write-around.
     */
    wordWrites: number;
    /**
     * Bytes read from memory: a line's bytes per line read.
     */
    bytesRead: number;
    /**
     * Bytes written to memory: a line's bytes per line written, and a
     * word's per word written.
     */
    bytesWritten: number;
}

/**
 * The transfer lane between the cache and memory.
 */
export interface BusView {
    /**
     * What moves.
     */
    transfer: Transfer;
    /**
     * The lane's label: "Fill · memory line 0x2A0 → set 2, way 1", or "Bus
     * idle".
     */
    text: string;
    /**
     * In timed mode, what the transfer costs: "20 cycles".
     */
    cyclesText?: string;
    /**
     * The line that moves.
     */
    line?: LineAddr;
}

/**
 * The way editor's form, as the student left it: the texts as typed, and
 * the valid and dirty switches.
 */
export interface WayForm {
    /**
     * Whether the way should hold a line.
     */
    valid: boolean;
    /**
     * The line address as typed; any byte of the line will do.
     */
    lineText: string;
    /**
     * Each data input's text as typed. An input the student has not typed
     * in is sent empty, and reads as the word the draft shows for it, which
     * follows the line typed.
     */
    wordTexts: string[];
    /**
     * Whether the line should be dirty.
     */
    dirty: boolean;
    /**
     * The rank the student chose; none until they choose one, while the
     * rank shown is the draft's result's.
     */
    rank?: number;
}

/**
 * True when a valid way holds the line, or with `present: false` when
 * none does; with `dirty`, when that way's dirty bit is as given.
 */
export interface LinePredicate {
    /**
     * Any byte address inside the line.
     */
    line: AddrLoc;
    /**
     * Whether the line is cached; true when not given.
     */
    present?: boolean;
    /**
     * Whether the cached line is dirty; `true` implies the line is cached.
     */
    dirty?: boolean;
}

/**
 * True when at least one predicate listed holds.
 */
export interface AnyPredicate {
    /**
     * The predicates; at least one.
     */
    any: Predicate[];
}

/**
 * True when every field given equals the way's: `tag` compares the tag's
 * value, not the line's address.
 */
export interface WayPredicate {
    /**
     * The way.
     */
    way: WayLoc;
    /**
     * Whether it holds a line.
     */
    valid?: boolean;
    /**
     * The tag of the line it holds.
     */
    tag?: Tag;
    /**
     * Whether its line is dirty.
     */
    dirty?: boolean;
    /**
     * Its place in its set's replacement order.
     */
    rank?: number;
}

/**
 * True when every predicate listed holds.
 */
export interface AllPredicate {
    /**
     * The predicates; at least one.
     */
    all: Predicate[];
}

/**
 * True when memory's word at the address, not the cache's copy, equals
 * the value.
 */
export interface MemoryPredicate {
    /**
     * The word's address, a multiple of the word size.
     */
    memory: AddrLoc;
    /**
     * The value, written in module JSON as a JSON number and read exactly
     * to 2^64 − 1.
     */
    equals: Word;
}

/**
 * True when the comparison holds for the counter.
 */
export interface CounterPredicate {
    /**
     * The counter.
     */
    counter: Counter;
    /**
     * How it compares.
     */
    op: CmpOp;
    /**
     * What it is compared with. A `u32`, as counts are: no counter of a
     * session comes near its end, and a module read back across the
     * boundary then holds no number JavaScript cannot represent.
     */
    value: number;
}

/**
 * True when the displayed run's configuration matches every field given.
 */
export interface ConfigPredicate {
    /**
     * The configuration wanted.
     */
    config: ConfigMatch;
}

/**
 * True when the displayed run's most recent completed access at or before
 * the cursor was made by an op made since the step began, and matches
 * every field given: what an instruct step waits for, an access made after
 * it began.
 */
export interface LastAccessPredicate {
    /**
     * The access wanted.
     */
    lastAccess: AccessMatch;
}

/**
 * True when the event the cursor rests on is of the kind, and, when an
 * entry is given, belongs to that entry of the displayed run: a step that
 * asks the student to drag the timeline back to step 1.2, the tag compare,
 * waits for `{"event": {"kind": "miss", "entry": 1}}`.
 */
export interface EventPredicate {
    /**
     * The event wanted.
     */
    event: EventMatch;
}

/**
 * True when the predicate given does not hold.
 */
export interface NotPredicate {
    /**
     * The predicate.
     */
    not: Predicate;
}

/**
 * True when the register holds the value.
 */
export interface RegisterPredicate {
    /**
     * The register, `r0` to `r7`.
     */
    register: Register;
    /**
     * The value, as for a memory predicate.
     */
    equals: Word;
}

/**
 * What Space or Shift+Space did. The page reads the new state afterwards,
 * so no event is returned.
 */
export type StepOutcome = { status: "moved" } | { status: "applied" } | { status: "idle"; message: string };

/**
 * What a change from the configuration bar, or a switch between Basics and
 * Advanced, did.
 */
export type ConfigChange = { status: "applied" } | { status: "needsReset"; config: Config; message: string };

/**
 * What a comparator shows for one way of the selected set.
 */
export type WayCompare = "pending" | "match" | "mismatch" | "invalid";

/**
 * What a store that hits does to memory.
 */
export type WritePolicy = "writeBack" | "writeThrough";

/**
 * What an edit changes: one way of the cache, or one word of memory.
 *
 * Serialized with its kind under `kind`: `{"kind":"way","set":2,"way":0}`
 * or `{"kind":"memory","addr":176}`. Its numbers are unchecked until the
 * edit is planned against the configuration in force. Read through a flat
 * form, as an [`Op`] is.
 */
export type EditTarget = { kind: "way"; set: SetIndex; way: WayIndex } | { kind: "memory"; addr: Addr };

/**
 * What an entry did, as its header's mark shows it.
 */
export type EntryOutcome = "hit" | "miss" | "edit" | "cpuOnly" | "program" | "error";

/**
 * What an entry is.
 */
export type EntryKind = "access" | "instruction" | "edit" | "program" | "error";

/**
 * What an event did.
 */
export type EventKind = "decode" | "hit" | "miss" | "victim" | "writeBack" | "writeThrough" | "fill" | "write" | "rank" | "memWrite" | "stall" | "complete" | "edit" | "error" | "evict" | "setup";

/**
 * What checking a prediction, or revealing it, found.
 */
export interface Feedback {
    /**
     * Its colour.
     */
    tone: FeedbackTone;
    /**
     * "Correct.", "Not quite." or "Shown.".
     */
    head: string;
    /**
     * The hint, or the explanation.
     */
    text: TextSegment[];
}

/**
 * What the CPU is doing.
 */
export type CpuStatus = { state: "noProgram" } | { state: "ready" } | { state: "busy"; entry: number; seq: number; of: number } | { state: "stalled"; remaining: number; total: number } | { state: "finished" } | { state: "faulted" } | { state: "outOfBudget" };

/**
 * What the Stats region shows at one point of a run.
 */
export interface Stats {
    /**
     * Every counter of the run.
     */
    counters: Counters;
    /**
     * The traffic to and from memory that the counters add up to.
     */
    traffic: Traffic;
    /**
     * The rows of the mode on screen, in order: accesses, hits, misses, the
     * hit rate and evictions; then in Basics the writes to memory, and in
     * Advanced the write-backs, write-throughs and write-arounds, the cycles
     * in timed mode, and the compulsory, capacity and conflict misses.
     */
    rows: StatRow[];
    /**
     * The share of accesses that hit: its value from 0 to 1, and its text a
     * whole percent, "67%". Absent before the run's first access.
     */
    hitRate?: Figure;
    /**
     * In timed mode, the average memory access time the miss rate
     * predicts: "AMAT = 1 + 0.40 × 20 = 9.0 cycles". As of the run's last
     * completed access: while an access is in flight, it is the figure
     * from before that access began. Absent untimed, and until the run's
     * first access has completed.
     */
    amat?: Figure;
    /**
     * In timed mode, what the run actually spent per access: "Measured:
     * 11.0 cycles per access (AMAT plus 2.0 for 1 write-back)." As of the
     * run's last completed access, like `amat`. Absent untimed, and until
     * the run's first access has completed.
     */
    measured?: Figure;
    /**
     * The sparkline: the outcomes of the run's latest accesses, oldest
     * first, at most `RECENT_OUTCOMES` of them. An access counts from its
     * tag compare on, as it does in `accesses`.
     */
    recent: Outcome[];
    /**
     * How many of those hit and missed: "1 hit, 2 misses", or, once the run
     * has had more accesses than the sparkline shows, "15 hits, 9 misses
     * (last 24)". Absent before the run's first access.
     */
    recentText?: string;
}

/**
 * What the address and value boxes would issue, and what the address strip
 * and the cache table show of the typed address while no access is at the
 * cursor.
 */
export interface IssueDraft {
    /**
     * The typed address, read.
     */
    address: AddressInput;
    /**
     * The typed value, read, for a store; absent for a load.
     */
    value?: WordInput;
    /**
     * The op an Issue press appends, when the address, and a store's
     * value, are valid.
     */
    op?: Op;
    /**
     * The request lane's label for the typed address: "Load 0x0A4", or "No
     * request" when it does not decode.
     */
    requestText: string;
    /**
     * The typed address's set, when it decodes.
     */
    selectedSet?: SetIndex;
    /**
     * One `=?` comparator per way of that set.
     */
    compare?: WayCompare[];
}

/**
 * What the page shows for a link or a file that cannot be opened, and the
 * ways forward that exist for it besides starting fresh.
 */
export interface LoadFailure {
    /**
     * "Could not open link", or "Could not open file".
     */
    title: string;
    /**
     * "This link doesn't describe a cache the Interactive Cache Explorer
     * can build."
     */
    lead: string;
    /**
     * Why, with the offending field in mono, and how many actions were not
     * replayed: `associativity: 3` then "is not a power of two. … The 6
     * actions after it were not replayed."
     */
    message: TextSegment[];
    /**
     * The document as pretty JSON, for "Show link contents"; the text as
     * it came when it is not JSON; absent when it could not be decoded.
     */
    contents?: string;
    /**
     * The same document with the configuration that failed repaired:
     * "Open with 2-way instead"; absent when a link cannot hold it, as for
     * a file larger than a link.
     */
    repair?: LoadOption;
    /**
     * The ops before the one that failed, opening at their end: "Open the
     * first 8 actions"; absent when a link cannot hold them.
     */
    prefix?: LoadOption;
}

/**
 * What the picker lists: one card per module, and its summary line.
 */
export interface ModuleIndexView {
    /**
     * The modules, in the index's order.
     */
    modules: ModuleCard[];
    /**
     * The picker's line while no module runs: "9 modules · none running".
     */
    summary: string;
}

/**
 * What the run has done so far. Every counter starts at 0 with the run.
 */
export interface Counters {
    /**
     * Loads and stores looked up in the cache.
     */
    accesses: number;
    /**
     * Loads looked up.
     */
    loads: number;
    /**
     * Stores looked up.
     */
    stores: number;
    /**
     * Accesses that found their line. With `misses`, it makes up
     * `accesses` at every event.
     */
    hits: number;
    /**
     * Accesses that did not find their line.
     */
    misses: number;
    /**
     * Lines that left the cache to make room for another.
     */
    evictions: number;
    /**
     * Dirty lines written back to memory.
     */
    writebacks: number;
    /**
     * Stores written through to memory under write-through.
     */
    writethroughs: number;
    /**
     * Stores that missed under write-no-allocate and went straight to
     * memory: write-arounds, under either write policy.
     */
    writearounds: number;
    /**
     * Lines brought in from memory.
     */
    fills: number;
    /**
     * Cycles spent, in timed mode; 0 otherwise.
     */
    cycles: Cycle;
    /**
     * Misses on a line never filled in this run. With `capacity` and
     * `conflict`, it makes up `misses` at every event.
     */
    compulsory: number;
    /**
     * The other misses that a fully associative LRU cache with as many
     * lines would also have: the run uses more lines than the cache holds.
     */
    capacity: number;
    /**
     * The other misses, which that cache would not have: caused by where
     * lines may go, and by which line the policy gives up.
     */
    conflict: number;
}

/**
 * What the transfer lane shows.
 */
export type Transfer = "idle" | "fill" | "writeBack" | "writeThrough";

/**
 * Where an entry or an event is, relative to the cursor.
 */
export type Tense = "past" | "current" | "future";

/**
 * Where each field lies in an address. The tag is never empty; the index is
 * absent when the cache is fully associative, and the offset when a line is
 * one byte.
 */
export interface BitRanges {
    /**
     * The tag's bits, the most significant ones.
     */
    tag: BitRange;
    /**
     * The index's bits, between the tag and the offset.
     */
    index?: BitRange;
    /**
     * The offset's bits, the least significant ones.
     */
    offset?: BitRange;
}

/**
 * Where the cursor is: its position, and the op, run, entry and event it
 * rests on.
 */
export interface Position {
    /**
     * The position: 0 before the first event, `k` after the `k`-th.
     */
    pos: EventPos;
    /**
     * How many ops are complete at this position (the facade's `cursor()`).
     */
    opsDone: number;
    /**
     * The op holding the position; none at position 0.
     */
    op?: OpIndex;
    /**
     * The run the position is in, from 1.
     */
    run: number;
    /**
     * The entry's number within the run; none for a setup and at position
     * 0.
     */
    entry?: number;
    /**
     * The event's number within its entry, from 1; 0 at position 0.
     */
    seq: number;
    /**
     * How many events the entry has; 0 at position 0.
     */
    entryLength: number;
    /**
     * The event's label, `5.3`; empty for a setup and at position 0.
     */
    label: string;
    /**
     * How much ends here.
     */
    boundary: Boundary;
    /**
     * Whether this is a run's origin: position 0, or a new run's `Setup`.
     */
    atRunOrigin: boolean;
}

/**
 * Whether a store that misses brings its line into the cache.
 */
export type AllocatePolicy = "writeAllocate" | "writeNoAllocate";

/**
 * Whether an access found its line: one mark of the hit and miss
 * sparkline. It also deserializes, because a module's `lastAccess`
 * predicate names one.
 */
export type Outcome = "hit" | "miss";

/**
 * Whether an access is in the cache.
 */
export type RequestStatus = "issued" | "none";

/**
 * Whether an access reads or writes memory.
 */
export type AccessKind = "read" | "write";

/**
 * Which controls are live.
 */
export interface Controls {
    /**
     * The live controls; every other is disabled.
     */
    enabled: ControlAction[];
    /**
     * The furthest position the cursor may reach, while a prediction is
     * unanswered.
     */
    seekLimit?: EventPos;
    /**
     * Why controls are disabled, while a prediction is unanswered.
     */
    guard?: string;
}

/**
 * Which field of an address.
 */
export type FieldKind = "tag" | "index" | "offset";

/**
 * Which row of the Stats region a [`StatRow`] is. The page labels the
 * rows; which rows there are, their order and their texts are the core's.
 */
export type StatKey = "accesses" | "hits" | "misses" | "hitRate" | "evictions" | "memoryWrites" | "writebacks" | "writethroughs" | "writearounds" | "cycles" | "compulsory" | "capacity" | "conflict";

/**
 * Which rule refused an edit.
 */
export type RefusalReason = "gated" | "sessionFull" | "linkFull" | "fileFull" | "midAccess" | "outOfRange" | "kindMismatch" | "badShape" | "valueTooWide" | "noChange" | "fieldsOnInvalidWay" | "missingLine" | "lineOutOfRange" | "duplicateLine" | "wrongSet" | "wrongSetFull" | "dirtyUnderWriteThrough" | "needsDirty" | "writeThroughMismatch" | "rankNotEditable" | "rankOutOfRange" | "misaligned";

/**
 * Which way a miss evicts once every way of its set is valid.
 */
export type Replacement = "lru" | "fifo" | "random";

/**
 * Why a configuration is invalid, as the page shows it: the field to
 * change, the message, and the nearest valid configuration.
 */
export interface ConfigErrorView {
    /**
     * The field the first broken rule is about: the one its repair
     * changes.
     */
    field: ConfigField;
    /**
     * The rule's message.
     */
    message: string;
    /**
     * The nearest valid configuration, with its action's label.
     */
    repair?: Repair;
}


/**
 * A guided module in progress: the module, its step and the student's
 * answers so far. Each call takes the session the module runs on.
 */
export class ModuleRun {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Checks the option selected, then the module panel. Correct lifts the
     * gate and applies the step's `then` ops; wrong makes "Show me"
     * available.
     *
     * # Errors
     *
     * Never in practice: the view always converts.
     */
    check(session: Session): ModuleView;
    /**
     * Leaves the module: lifts its gate from `session`, and leaves the
     * workbench as it is.
     */
    exit(session: Session): void;
    /**
     * A run of the module read from `json`, on its first step and not yet
     * started.
     *
     * # Errors
     *
     * Throws where the module breaks its format, and how.
     */
    static fromJson(json: string): ModuleRun;
    /**
     * The module.
     *
     * # Errors
     *
     * Never in practice: a module always converts.
     */
    module(): Module;
    /**
     * Next: goes on from an explanation or an answered prediction, or
     * starts the module again from the done screen. Then the module panel.
     *
     * # Errors
     *
     * Never in practice: the view always converts.
     */
    next(session: Session): ModuleView;
    /**
     * Selects option `option` of the unanswered prediction, from 0.
     */
    select(option: number): void;
    /**
     * "Show me": applies an instruct step's ops, moves the cursor to where
     * an observe step's condition holds, or reveals a prediction's answer
     * after a wrong one. Then the module panel.
     *
     * # Errors
     *
     * Never in practice: the view always converts.
     */
    showMe(session: Session): ModuleView;
    /**
     * Starts the module on `session`, from its first step, and returns the
     * events of the ops it appends.
     *
     * # Errors
     *
     * Throws why the session refuses those ops; the session and the run are
     * then as they were.
     */
    start(session: Session): MicroEvents;
    /**
     * Re-evaluates the current step after any change to `session`, and
     * sets or lifts its gate; at most one step completes. Then the module
     * panel.
     *
     * # Errors
     *
     * Never in practice: the view always converts.
     */
    sync(session: Session): ModuleView;
    /**
     * What the module panel draws, with `session` as it is.
     *
     * # Errors
     *
     * Never in practice: the view always converts.
     */
    view(session: Session): ModuleView;
}

/**
 * A run of the Interactive Cache Explorer: the log of ops over a first
 * configuration, the events they produced, and the cursor on those events.
 */
export class Session {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Appends `op` at the cursor, dropping the undone ops after it, and
     * returns its events; the cursor goes to its last event.
     *
     * # Errors
     *
     * Throws why the op is refused, leaving the session unchanged.
     */
    applyOp(op: Op): MicroEvents;
    /**
     * Everything the configuration bar draws for `shown`, the pending
     * configuration or the displayed run's.
     *
     * # Errors
     *
     * Throws when `shown` is not a configuration.
     */
    configControls(shown: Config): ConfigControls;
    /**
     * How many ops are done at the cursor.
     */
    cursor(): number;
    /**
     * `addr` split into its tag, index and offset under the displayed
     * run's configuration.
     *
     * # Errors
     *
     * Throws when `addr` does not fit the address width.
     */
    decode(addr: number): Decoded;
    /**
     * The way editor's draft of `form` for way `way` of set `set`.
     *
     * # Errors
     *
     * Throws when `set`, `way` or `form` is not of its type.
     */
    draftEdit(set: SetIndex, way: WayIndex, form: WayForm): EditDraft;
    /**
     * What the address and value boxes would issue on the displayed run.
     *
     * # Errors
     *
     * Throws when `kind` is not an access kind.
     */
    draftIssue(kind: AccessKind, addrText: string, valueText: string): IssueDraft;
    /**
     * The memory editor's draft of `valueText` for the word at `addr`.
     *
     * # Errors
     *
     * Throws when `addr` is not an address.
     */
    draftMemoryEdit(addr: Addr, valueText: string): MemoryEditDraft;
    /**
     * The displayed run's entries from its `first`, at most `count` of
     * them.
     *
     * # Errors
     *
     * Never in practice: entries always convert.
     */
    entries(first: number, count: number): EntryViews;
    /**
     * The events whose ids run from `start` up to but not including `end`,
     * the undone ones after the cursor included.
     *
     * # Errors
     *
     * Never in practice: events always convert.
     */
    eventsRange(start: number, end: number): MicroEvents;
    /**
     * Every event up to the cursor. The log reads windows with
     * `eventsRange` instead.
     *
     * # Errors
     *
     * Never in practice: events always convert.
     */
    events(): MicroEvents;
    /**
     * The first position of the displayed run where `p` holds.
     *
     * # Errors
     *
     * Throws when `p` is not a predicate.
     */
    findEvent(p: Predicate): EventPos | undefined;
    /**
     * A session on `Config::first_visit()`, the default configuration with
     * pattern memory: the page's first visit, and "Start fresh".
     *
     * # Errors
     *
     * Never in practice: that configuration is valid.
     */
    static firstVisit(): Session;
    /**
     * The session a saved file's JSON holds, replayed at once.
     *
     * # Errors
     *
     * Throws the message of why the file cannot be opened.
     */
    static fromJson(json: string): Session;
    /**
     * The session a share string holds, replayed at once. The page opens
     * links with `ShareLoad`, a slice at a time.
     *
     * # Errors
     *
     * Throws the message of why the link cannot be opened.
     */
    static fromShareString(s: string): Session;
    /**
     * The memory table's rows from its `first`, at most `count` of them.
     *
     * # Errors
     *
     * Never in practice: rows always convert.
     */
    memoryLines(first: number, count: number): MemoryLines;
    /**
     * A session on `Config::default()`, with an empty log.
     *
     * # Errors
     *
     * Never in practice: the default configuration is valid.
     */
    constructor();
    /**
     * How many ops the log holds, the undone ones included.
     */
    opCount(): number;
    /**
     * `source` parsed as a program or a trace under the displayed run's
     * configuration.
     *
     * # Errors
     *
     * Throws when `format` is not a program format.
     */
    parseProgram(source: string, format: ProgramFormat): ParseResult;
    /**
     * The plan of `patch` on `target` at the cursor, or why it is refused.
     *
     * # Errors
     *
     * Throws when `target` or `patch` is not of its type.
     */
    planEdit(target: EditTarget, patch: EditPatch): EditOutcome;
    /**
     * The cursor's position, with its place in the run and the op.
     *
     * # Errors
     *
     * Never in practice: the position always converts.
     */
    position(): Position;
    /**
     * A change from the configuration bar: applied at once, or the
     * question "Reset to apply?" with the configuration to apply.
     *
     * # Errors
     *
     * Throws when `config` is not a configuration, or why the new run's op
     * would be refused.
     */
    proposeConfig(config: Config): ConfigChange;
    /**
     * A typed address, read against the displayed run's geometry.
     *
     * # Errors
     *
     * Never in practice: the reading always converts.
     */
    readAddress(text: string): AddressInput;
    /**
     * A typed hit time or miss penalty, applied to `shown`.
     *
     * # Errors
     *
     * Throws when `field` or `shown` is not of its type.
     */
    readConfigNumber(field: ConfigNumberField, text: string, shown: Config): ConfigNumberInput;
    /**
     * A typed value, read against the displayed run's word size.
     *
     * # Errors
     *
     * Never in practice: the reading always converts.
     */
    readWord(text: string): WordInput;
    /**
     * Moves the cursor forward to the next undo stop; false if there is
     * none.
     */
    redo(): boolean;
    /**
     * Moves the cursor to where `opIndex` ops are done: the end of op
     * `opIndex - 1`, or position 0 for 0; past the log, to its end.
     */
    scrubTo(opIndex: number): void;
    /**
     * Moves the cursor to the position `pos`, as far as the log and a
     * module's unanswered prediction allow.
     *
     * # Errors
     *
     * Throws when `pos` is not a position.
     */
    seek(pos: EventPos): void;
    /**
     * Switches between Basics and Advanced, which never resets the run.
     *
     * # Errors
     *
     * Never in practice: the result always converts.
     */
    setAdvanced(on: boolean): ConfigChange;
    /**
     * Replaces the lines the student asked to see, the "Jump to" rows.
     *
     * # Errors
     *
     * Throws when `lines` is not a list of line addresses.
     */
    setMemoryWatch(lines: LineAddr[]): void;
    /**
     * The run as Share describes it.
     *
     * # Errors
     *
     * Never in practice: the summary always converts.
     */
    shareSummary(): ShareSummary;
    /**
     * What the page draws at the cursor.
     *
     * # Errors
     *
     * Never in practice: the snapshot always converts.
     */
    state(): State;
    /**
     * The Stats region at the cursor.
     *
     * # Errors
     *
     * Never in practice: the stats always convert.
     */
    stats(): Stats;
    /**
     * Shift+Space: moves the cursor to the end of the op it is in, or of
     * the next one; at the end of the log, loads `program` as `stepEvent`
     * does, then executes instructions until one memory access completes.
     *
     * # Errors
     *
     * Throws when `program` is not a program source.
     */
    stepAccess(program?: ProgramSource | null): StepOutcome;
    /**
     * Space: moves the cursor one event on, or at the end of the log
     * executes one instruction and rests on its first event. `program`, the
     * drawer's text, is loaded first, as an op of its own, when it differs
     * from the program loaded.
     *
     * # Errors
     *
     * Throws when `program` is not a program source.
     */
    stepEvent(program?: ProgramSource | null): StepOutcome;
    /**
     * One byte per step of the displayed run, in order, for the scrubber's
     * ticks. Bit 0 (1) is set on an entry's first step and bit 1 (2) on
     * every step of an entry that missed, so a tick is 0, 1, 2 or 3. The
     * loading summary's `ticks` hold the same bytes as a `number[]`.
     */
    ticks(): Uint8Array;
    /**
     * The displayed run's timeline: the scrubber, the log's header and the
     * live region's sentence.
     *
     * # Errors
     *
     * Never in practice: the timeline always converts.
     */
    timeline(): Timeline;
    /**
     * The session as a saved file holds it: compact JSON, the text a share
     * string deflates, within the size a file may have, so it always opens.
     */
    toJson(): string;
    /**
     * The session as a share string, the text after `#s=`.
     *
     * # Errors
     *
     * Throws, with the message Share shows, when the string is too long
     * for a link to open; `toJson` still holds the run.
     */
    toShareString(): string;
    /**
     * Moves the cursor back to the previous undo stop; false if there is
     * none.
     */
    undo(): boolean;
    /**
     * A session whose first run has `config`.
     *
     * # Errors
     *
     * Throws when `config` is not a configuration, or the message of the
     * first rule it breaks.
     */
    static withConfig(config: Config): Session;
}

/**
 * A share string or a saved file being opened: checked at once, then
 * replayed a slice at a time, so the page can draw between slices.
 */
export class ShareLoad {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Replays the next slice of about `LOAD_SLICE_EVENTS` events, ending at
     * an entry boundary. Once the load is done or has failed, every call
     * answers the same.
     *
     * # Errors
     *
     * Never in practice: the progress always converts.
     */
    advance(): LoadProgress;
    /**
     * Opens a saved file's JSON, as `fromShareString` opens a link.
     */
    static fromJson(json: string): ShareLoad;
    /**
     * Opens a share string. A link that cannot be opened is reported by
     * `advance`.
     */
    static fromShareString(s: string): ShareLoad;
    /**
     * The loaded session, its cursor where the document placed it.
     *
     * # Errors
     *
     * Throws why the load failed, before `advance` has returned `"done"`,
     * and once the session has been taken.
     */
    takeSession(): Session;
}

/**
 * The build: the crates' version and the share format.
 *
 * # Errors
 *
 * Never in practice: the build information always converts.
 */
export function buildInfo(): BuildInfo;

/**
 * Whether `p` holds at `session`'s cursor.
 *
 * # Errors
 *
 * Throws when `p` is not a predicate.
 */
export function evalPredicate(session: Session, p: Predicate): boolean;

/**
 * The module `json` holds, read and checked.
 *
 * # Errors
 *
 * Throws where the module breaks its format, and how.
 */
export function parseModule(json: string): Module;

/**
 * The module picker for the module index `json` holds.
 *
 * # Errors
 *
 * Throws where the index breaks its format, and how.
 */
export function parseModuleIndex(json: string): ModuleIndexView;

/**
 * `config` checked on its own: its summary and geometry, or the first rule
 * it breaks with its repair.
 *
 * # Errors
 *
 * Throws when `config` is not a configuration.
 */
export function validateConfig(config: Config): ConfigCheck;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_modulerun_free: (a: number, b: number) => void;
    readonly __wbg_session_free: (a: number, b: number) => void;
    readonly __wbg_shareload_free: (a: number, b: number) => void;
    readonly buildInfo: () => [number, number, number];
    readonly evalPredicate: (a: number, b: any) => [number, number, number];
    readonly modulerun_check: (a: number, b: number) => [number, number, number];
    readonly modulerun_exit: (a: number, b: number) => void;
    readonly modulerun_fromJson: (a: number, b: number) => [number, number, number];
    readonly modulerun_module: (a: number) => [number, number, number];
    readonly modulerun_next: (a: number, b: number) => [number, number, number];
    readonly modulerun_select: (a: number, b: number) => void;
    readonly modulerun_showMe: (a: number, b: number) => [number, number, number];
    readonly modulerun_start: (a: number, b: number) => [number, number, number];
    readonly modulerun_sync: (a: number, b: number) => [number, number, number];
    readonly modulerun_view: (a: number, b: number) => [number, number, number];
    readonly parseModule: (a: number, b: number) => [number, number, number];
    readonly parseModuleIndex: (a: number, b: number) => [number, number, number];
    readonly session_applyOp: (a: number, b: any) => [number, number, number];
    readonly session_configControls: (a: number, b: any) => [number, number, number];
    readonly session_cursor: (a: number) => number;
    readonly session_decode: (a: number, b: number) => [number, number, number];
    readonly session_draftEdit: (a: number, b: any, c: any, d: any) => [number, number, number];
    readonly session_draftIssue: (a: number, b: any, c: number, d: number, e: number, f: number) => [number, number, number];
    readonly session_draftMemoryEdit: (a: number, b: any, c: number, d: number) => [number, number, number];
    readonly session_entries: (a: number, b: number, c: number) => [number, number, number];
    readonly session_events: (a: number) => [number, number, number];
    readonly session_eventsRange: (a: number, b: number, c: number) => [number, number, number];
    readonly session_findEvent: (a: number, b: any) => [number, number, number];
    readonly session_firstVisit: () => [number, number, number];
    readonly session_fromJson: (a: number, b: number) => [number, number, number];
    readonly session_fromShareString: (a: number, b: number) => [number, number, number];
    readonly session_memoryLines: (a: number, b: number, c: number) => [number, number, number];
    readonly session_new: () => [number, number, number];
    readonly session_opCount: (a: number) => number;
    readonly session_parseProgram: (a: number, b: number, c: number, d: any) => [number, number, number];
    readonly session_planEdit: (a: number, b: any, c: any) => [number, number, number];
    readonly session_position: (a: number) => [number, number, number];
    readonly session_proposeConfig: (a: number, b: any) => [number, number, number];
    readonly session_readAddress: (a: number, b: number, c: number) => [number, number, number];
    readonly session_readConfigNumber: (a: number, b: any, c: number, d: number, e: any) => [number, number, number];
    readonly session_readWord: (a: number, b: number, c: number) => [number, number, number];
    readonly session_redo: (a: number) => number;
    readonly session_scrubTo: (a: number, b: number) => void;
    readonly session_seek: (a: number, b: any) => [number, number];
    readonly session_setAdvanced: (a: number, b: number) => [number, number, number];
    readonly session_setMemoryWatch: (a: number, b: any) => [number, number];
    readonly session_shareSummary: (a: number) => [number, number, number];
    readonly session_state: (a: number) => [number, number, number];
    readonly session_stats: (a: number) => [number, number, number];
    readonly session_stepAccess: (a: number, b: number) => [number, number, number];
    readonly session_stepEvent: (a: number, b: number) => [number, number, number];
    readonly session_ticks: (a: number) => [number, number];
    readonly session_timeline: (a: number) => [number, number, number];
    readonly session_toJson: (a: number) => [number, number];
    readonly session_toShareString: (a: number) => [number, number, number, number];
    readonly session_undo: (a: number) => number;
    readonly session_withConfig: (a: any) => [number, number, number];
    readonly shareload_advance: (a: number) => [number, number, number];
    readonly shareload_fromJson: (a: number, b: number) => number;
    readonly shareload_fromShareString: (a: number, b: number) => number;
    readonly shareload_takeSession: (a: number) => [number, number, number];
    readonly validateConfig: (a: any) => [number, number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
