The lex.include Annotation

Introduction

    `:: include src="..." ::` is a reserved annotation that pulls another Lex file's content into the host document at parse-plus-resolution time. It is not a new element — it uses the standard annotation surface — but its canonical label `lex.include` is reserved by the core, and the resolver in `lex_core::lex::includes` gives it specific behaviour at resolution time.

    Two distinct spellings reach this label: `include` (the user-facing shortcut, used throughout this document) and `lex.include` (the canonical, transmitted on the extension wire). The prefix-strip rule formally also produces `include` from the canonical, but since the local part is one segment, it coincides with the shortcut form rather than introducing a third spelling. See [../general.lex#4] for the full label namespace model.

    Authors: this is the canonical reference for the *behaviour* of an include. The original design rationale lives in `specs/proposals/done/includes.lex`; that document is frozen and kept for historical context.

    Tooling: the `lex.*` annotation label prefix is reserved for core-defined semantics. Third-party tooling must not author labels in this namespace; the core may add new `lex.*` labels without coordinating with downstream.

Syntax

    Always the marker form of an annotation, with a mandatory `src` parameter:

        :: include src="chapters/01.lex" ::

    The annotation may appear anywhere a regular annotation is legal: at document root, inside a session, inside a definition, inside a list item, or inside another annotation's block content.

    Parameters

        - `src` (required): the path to include, as a quoted string. Two forms:

            - Relative path (`chapters/01.lex`, `./shared/header.lex`): resolves against the directory of the file that contains the include.
            - Root-absolute path (leading `/`, e.g. `/shared/header.lex`): resolves against the resolution root (see Resolution Root below).

        - No other parameters carry semantic meaning today.

Resolution Behaviour

    The annotation is parsed exactly like any other annotation. Its expansion is a separate post-parse pass that the resolver runs against the parsed AST. This pass:

    1. Walks the tree, finding every `lex.include` annotation.
    2. For each, resolves the `src` parameter to a canonical path under the resolution root.
    3. Loads the target file (via an injected `Loader`; production loader is `FsLoader`).
    4. Recursively resolves the loaded file's own includes.
    5. Stamps each loaded node's `Range.origin_path` with the loaded file's path.
    6. Splices the loaded file's body into the parent container at the include's position.
    7. Converts the included document's title and document-level annotations to non-document forms (DocumentTitle → Paragraph, doc-level annotations → regular annotations) and prepends them to the splice. This matches what a textual paste with indent-shift would parse: an unindented title line becomes a paragraph in the host's context; doc-level annotations become next-sibling-attaching ones.

    Standard annotation attachment then runs on the merged tree. The `lex.include` annotation lands on the first spliced sibling per the normal "attach to next sibling" rule.

    The mental model authors should hold: an include behaves as if the file's text had been pasted at the include site, indent-shifted to match. The implementation operates on parsed trees rather than raw text, but the resulting tree is the same.

Resolution Root

    The resolution root is the directory all include paths must canonicalize within. Discovery order:

    1. An explicit override (`--includes-root` CLI flag or `[includes].root` in `.lex.toml`).
    2. The directory of the nearest `.lex.toml` walking upward from the entry-point file.
    3. The entry-point file's own directory.

    Any include path that lexically normalizes outside the root is a `RootEscape` error, even if the on-disk file exists.

Container Policy

    Lex's typed-content system enforces that certain containers do not allow Sessions: `Definition`, `Annotation` body, and `ListItem` (collectively, `GeneralContainer`). When a `lex.include` site sits inside one of these and the included file contains top-level Sessions, resolution fails with a `ContainerPolicy` error pointing at the include site.

    Includes at the document root or inside a Session never violate, because `SessionContainer` accepts every element type.

Errors

    The resolver can produce these errors, all carrying the offending path or include site:

    - `Cycle`: an include chain looped back on itself. The resolver tracks the active chain of canonicalized paths; a path already on the chain is the cycle.
    - `DepthExceeded`: the include depth exceeded `[includes].max_depth` (default 8).
    - `RootEscape`: the resolved path is outside the resolution root.
    - `NotFound`: the loader could not find the target file.
    - `LoaderIo`: the loader propagated a non-`NotFound` I/O error (permission denied, broken symlink, etc.).
    - `ParseFailed`: the loaded file did not parse as Lex.
    - `ContainerPolicy`: the included content is illegal in the host container (see above).
    - `MissingSrc`: the annotation had no `src` parameter.

CLI

    `lex convert` and `lex inspect` resolve includes by default. `lex format` never expands includes — the annotation stays in the formatted output as authored.

    Flags:

    - `--no-includes`: disable resolution and operate on the unresolved tree (useful for inspecting a single document atom in isolation).
    - `--includes-root <PATH>`: explicit root override.

LSP

    `lex-lsp` resolves includes on `did_open` and `did_change` for diagnostic purposes. Each `IncludeError` variant maps to a distinct diagnostic code (`include-cycle`, `include-depth-exceeded`, `include-root-escape`, `include-not-found`, `include-parse-failed`, `include-container-policy`, `include-loader-io`, `include-missing-src`).

    Editor UX:

    - Goto-definition on the include annotation jumps to the target file (lands at the file head). Returns no result if the target doesn't exist on disk — the `include-not-found` diagnostic surfaces the underlying problem.
    - Hover on the include annotation shows a small markdown preview: source path + resolved path + first two non-blank lines of the target.

Out of Scope

    The following are deliberately not part of the core. Some may land in future versions; some are forever no.

    - URLs (`src="https://..."`): deferred. Async I/O, caching, offline builds, and exfiltration risk all have to be designed before this can ship safely.
    - Absolute filesystem paths (`src="/home/user/..."`): never. Root-absolute paths (under the resolution root) cover the legitimate use cases without the security and portability issues.
    - Conditional includes (`if=`, `unless=`, `when=`): never. Conditional logic belongs in a build system.
    - Variable substitution / templating: never. Lex is not a template language.
    - Shell commands as src: never. Arbitrary code execution at resolution time is an attack vector with no legitimate core use case.
    - Partial includes (lines 10-20, only session 2.1): not in v1. Better atomization is the substitute — split the file.
    - Cross-document references (`[book#2.3]` after `:: include src="book.lex" as="book" ::`): not in v1. Needs a namespace-design pass of its own.

Examples

    Most common case — splicing chapter files into a top-level book:

        Book Title

            :: include src="chapters/01.lex" ::

            :: include src="chapters/02.lex" ::

            :: include src="chapters/03.lex" ::

    Inside a session:

        2. Appendix

            :: include src="appendix/glossary.lex" ::

    Including a fragment with no top-level sessions (a thread of review annotations, for instance):

        1. Introduction

            Some content.

            :: include src="reviews/intro-thread.lex" ::

    Root-absolute path (resolves under the project root, regardless of the host file's depth):

        :: include src="/shared/standard-header.lex" ::

Learn More

    - Annotation surface: `specs/elements/annotation.lex`
    - Original proposal (frozen): `specs/proposals/done/includes.lex`
    - Reference fixtures: `specs/elements/lex.include.docs/`
