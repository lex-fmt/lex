#!/usr/bin/env node
/**
 * CST-to-JSON bridge: converts tree-sitter XML output to the canonical
 * JSON format produced by `lex inspect ast-json`.
 *
 * Usage:
 *   npx tree-sitter parse -x file.lex | node scripts/cst-to-json.js
 *   node scripts/cst-to-json.js < cst.xml
 *
 * The output JSON matches the schema from crates/lex-cli/src/transforms.rs
 * so it can be diff'd against `lex inspect ast-json` for parity testing.
 */

const { execSync } = require("child_process");

// Simple XML parser — tree-sitter's XML is well-formed and simple enough
// that we don't need a full DOM parser.
function parseXML(xml) {
  const nodes = [];
  const stack = [];
  let pos = 0;

  // Skip XML declaration
  if (xml.startsWith("<?xml")) {
    pos = xml.indexOf("?>") + 2;
    while (pos < xml.length && xml[pos] === "\n") pos++;
  }

  while (pos < xml.length) {
    if (xml[pos] === "<") {
      if (xml[pos + 1] === "/") {
        // Closing tag
        const end = xml.indexOf(">", pos);
        const parent = stack.pop();
        if (stack.length > 0) {
          stack[stack.length - 1].children.push(parent);
        } else {
          nodes.push(parent);
        }
        pos = end + 1;
      } else {
        // Opening tag
        const end = xml.indexOf(">", pos);
        const tagContent = xml.substring(pos + 1, end);
        const selfClosing = tagContent.endsWith("/");
        const clean = selfClosing
          ? tagContent.slice(0, -1).trim()
          : tagContent.trim();

        // Parse tag name and attributes
        const spaceIdx = clean.indexOf(" ");
        const tagName = spaceIdx === -1 ? clean : clean.substring(0, spaceIdx);
        const attrs = {};
        if (spaceIdx !== -1) {
          const attrStr = clean.substring(spaceIdx + 1);
          const attrRe = /(\w+)="([^"]*)"/g;
          let m;
          while ((m = attrRe.exec(attrStr)) !== null) {
            attrs[m[1]] = m[2];
          }
        }

        const node = { tag: tagName, attrs, children: [], text: "" };

        if (selfClosing) {
          if (stack.length > 0) {
            stack[stack.length - 1].children.push(node);
          } else {
            nodes.push(node);
          }
        } else {
          stack.push(node);
        }
        pos = end + 1;
      }
    } else {
      // Text content — collect until next <
      const nextTag = xml.indexOf("<", pos);
      if (nextTag === -1) break;
      const text = xml.substring(pos, nextTag);
      if (stack.length > 0 && text.length > 0) {
        stack[stack.length - 1].text += text;
      }
      pos = nextTag;
    }
  }

  return nodes[0]; // root document node
}

// Decode XML entities
function decodeXML(text) {
  return text
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'");
}

// Extract text content from a node.
// Only leaf nodes (no children) carry real text; non-leaf text is XML whitespace.
function extractText(node) {
  if (!node) return "";
  if (node.children.length === 0) return decodeXML(node.text || "");
  let result = "";
  for (const child of node.children) {
    result += extractText(child);
  }
  return result;
}

// Extract the field value from a node's children (e.g., field="title")
function findField(node, fieldName) {
  for (const child of node.children) {
    if (child.attrs.field === fieldName) return child;
  }
  return null;
}

// Extract text from a subject line, stripping the trailing colon
function extractSubject(node) {
  const text = extractText(node);
  // subject_content includes trailing colon; strip it to match lex AST
  return text.replace(/:$/, "");
}

// Extract title text from a session's title field.
// Lex AST preserves trailing colon in session titles, so we keep it.
function extractTitle(node) {
  const titleNode = findField(node, "title");
  if (!titleNode) return "";
  return extractText(titleNode);
}

// Convert blank_line nodes into BlankLineGroup nodes (one per blank line,
// matching lex-cli which emits separate BlankLineGroup(count=1) per line)
function groupBlanks(children) {
  const result = [];
  for (const child of children) {
    if (child.tag === "blank_line") {
      result.push({ type: "BlankLineGroup", count: 1 });
    } else {
      result.push(child);
    }
  }
  return result;
}

// Filter out blank_line nodes that are structural (session separators, etc.)
// and convert block children to canonical JSON
function convertBlockChildren(node) {
  const grouped = groupBlanks(node.children);
  const converted = grouped
    .map((child) => {
      if (child.type === "BlankLineGroup") return child;
      return convertNode(child);
    })
    .filter((n) => n !== null);

  // Remove BlankLineGroups immediately before VerbatimBlocks
  // (lex-cli absorbs leading blank lines into the verbatim block)
  const result = [];
  for (let i = 0; i < converted.length; i++) {
    if (
      converted[i] &&
      converted[i].type === "BlankLineGroup" &&
      converted[i + 1] &&
      converted[i + 1].type === "VerbatimBlock"
    ) {
      continue; // skip the BlankLineGroup
    }
    result.push(converted[i]);
  }
  return result;
}

// Extract list marker from list_item_line text (e.g., "- First item" → "-")
function extractListMarker(text) {
  // Dash marker
  if (text.startsWith("- ")) return "-";

  // Parenthetical: (1), (a), (IV)
  const parenMatch = text.match(/^\(([^)]+)\)\s/);
  if (parenMatch) return `(${parenMatch[1]})`;

  // Numbered/alpha/roman: 1. or 1) or a. or IV.
  const orderedMatch = text.match(/^([0-9a-zA-Z.]+[.)]) /);
  if (orderedMatch) return orderedMatch[1];

  return "";
}

// Extract list item text (everything after marker)
function extractListItemText(text) {
  // Dash marker
  if (text.startsWith("- ")) return text.substring(2);

  // Parenthetical
  const parenMatch = text.match(/^\([^)]+\)\s(.*)/);
  if (parenMatch) return parenMatch[1];

  // Numbered/alpha/roman
  const orderedMatch = text.match(/^[0-9a-zA-Z.]+[.)]\s(.*)/);
  if (orderedMatch) return orderedMatch[1];

  return text;
}

// Annotation attachment: remove Annotation blocks from a children array and
// attach them to their nearest sibling element (by blank-line distance).
// Returns { annotations: [...], children: [...] } where annotations are
// container-level (no valid attachment target) and children has annotations
// removed.
function attachAnnotationsClean(blocks) {
  // Phase 1: identify annotations and their attachment targets
  const isContent = (b) => b && b.type !== "Annotation" && b.type !== "BlankLineGroup";

  // Build list of content element indices
  const contentIndices = [];
  for (let i = 0; i < blocks.length; i++) {
    if (isContent(blocks[i])) contentIndices.push(i);
  }

  const containerAnnotations = [];
  const attachments = new Map(); // target index → [annotations]

  for (let i = 0; i < blocks.length; i++) {
    if (!blocks[i] || blocks[i].type !== "Annotation") continue;

    // Find previous content element and count blank lines between
    let prevIdx = -1;
    let blanksBefore = 0;
    for (let j = i - 1; j >= 0; j--) {
      if (isContent(blocks[j])) { prevIdx = j; break; }
      if (blocks[j] && blocks[j].type === "BlankLineGroup") blanksBefore += (blocks[j].count || 1);
    }

    // Find next content element and count blank lines between
    let nextIdx = -1;
    let blanksAfter = 0;
    for (let j = i + 1; j < blocks.length; j++) {
      if (isContent(blocks[j])) { nextIdx = j; break; }
      if (blocks[j] && blocks[j].type === "BlankLineGroup") blanksAfter += (blocks[j].count || 1);
    }

    let targetIdx = -1;
    if (prevIdx >= 0 && nextIdx >= 0) {
      targetIdx = blanksBefore < blanksAfter ? prevIdx : nextIdx; // tie → next
    } else if (nextIdx >= 0) {
      // No previous content — could be document-level or attach to next
      // At document level, leading annotations are document-level
      containerAnnotations.push(blocks[i]);
      continue;
    } else if (prevIdx >= 0) {
      // No next content — container-level
      containerAnnotations.push(blocks[i]);
      continue;
    } else {
      containerAnnotations.push(blocks[i]);
      continue;
    }

    if (!attachments.has(targetIdx)) attachments.set(targetIdx, []);
    attachments.get(targetIdx).push(blocks[i]);
  }

  // Phase 2: build result without annotations, attaching where needed
  const children = [];
  for (let i = 0; i < blocks.length; i++) {
    if (!blocks[i]) continue;
    if (blocks[i].type === "Annotation") continue; // skip — already handled
    children.push(blocks[i]);
    if (attachments.has(i)) {
      if (!blocks[i].annotations) blocks[i].annotations = [];
      blocks[i].annotations.push(...attachments.get(i));
    }
  }

  return { annotations: containerAnnotations, children };
}

// Convert a single CST node to canonical JSON
function convertNode(node) {
  if (!node || !node.tag) return null;

  switch (node.tag) {
    case "document": {
      let blocks = convertBlockChildren(node);
      // Strip trailing BlankLineGroup (tree-sitter emits blank_line for
      // every trailing newline; lex AST does not include these)
      while (
        blocks.length > 0 &&
        blocks[blocks.length - 1] &&
        blocks[blocks.length - 1].type === "BlankLineGroup"
      ) {
        blocks.pop();
      }

      // Document title detection BEFORE annotation attachment.
      // Title = first paragraph followed by BlankLineGroup,
      // NOT followed by a container (session/definition).
      // Must check original structure so annotation removal doesn't
      // create false positives.
      let title = "";
      if (blocks.length >= 2) {
        const first = blocks[0];
        const second = blocks[1];
        const third = blocks[2];
        const isTitle =
          first &&
          first.type === "Paragraph" &&
          first.lines &&
          first.lines.length >= 1 &&
          second &&
          second.type === "BlankLineGroup" &&
          // NOT followed by a container (session or definition)
          (!third ||
            (third.type !== "Session" && third.type !== "Definition"));

        if (isTitle) {
          title = first.lines.map((l) => l.content).join("\n");
          // Remove the title paragraph and its trailing blank
          blocks.splice(0, 2);
        }
      }

      // Annotation attachment: remove annotations from children and
      // attach to nearest sibling or collect as container-level
      const attached = attachAnnotationsClean(blocks);
      const annotations = attached.annotations;
      const children = attached.children;

      const doc = {
        type: "Document",
        title,
        children,
      };

      if (annotations.length > 0) {
        doc.annotations = annotations;
      }

      // Clean up internal markers
      for (const child of children) {
        delete child._hasSubject;
        delete child._subjectText;
      }

      return doc;
    }

    case "session": {
      const title = extractTitle(node);
      const blocks = convertBlockChildren(node);
      // Skip structural blank lines between title and first content block.
      // Keep blank lines between content blocks (lex AST preserves these).
      let startIdx = 0;
      while (
        startIdx < blocks.length &&
        blocks[startIdx] &&
        blocks[startIdx].type === "BlankLineGroup"
      ) {
        startIdx++;
      }
      const trimmed = blocks.slice(startIdx).filter((b) => b !== null);

      // Annotation attachment within session
      const attached = attachAnnotationsClean(trimmed);
      const children = attached.children;

      const session = {
        type: "Session",
        title,
        children,
      };

      if (attached.annotations.length > 0) {
        session.annotations = attached.annotations;
      }

      // Clean internal markers
      for (const child of children) {
        delete child._hasSubject;
        delete child._subjectText;
        delete child._isTitle;
      }

      return session;
    }

    case "definition": {
      const subjectNode = findField(node, "subject");
      const subject = subjectNode ? extractSubject(subjectNode) : "";
      const blocks = convertBlockChildren(node);

      return {
        type: "Definition",
        subject,
        children: blocks.filter((b) => b !== null),
      };
    }

    case "list": {
      const items = node.children
        .filter((c) => c.tag === "list_item")
        .map(convertNode)
        .filter((n) => n !== null);

      return {
        type: "List",
        items,
      };
    }

    case "list_item": {
      const lineNode = node.children.find((c) => c.tag === "list_item_line");
      const lineText = lineNode ? extractText(lineNode) : "";
      const marker = extractListMarker(lineText);
      const text = extractListItemText(lineText);

      // Get nested children (blocks inside indented list item content)
      const nestedBlocks = convertBlockChildren({
        children: node.children.filter(
          (c) => c.tag !== "list_item_line" && c.tag !== "blank_line",
        ),
      });

      return {
        type: "ListItem",
        marker,
        text: [text + "\n"],
        children: nestedBlocks.filter((b) => b !== null),
      };
    }

    case "paragraph": {
      const lines = node.children
        .filter((c) => c.tag === "text_line")
        .map((tl) => {
          // Strip leading indentation — lex AST stores content without indent
          const content = extractText(tl).replace(/^[ \t]+/, "");
          return { type: "TextLine", content };
        });

      const para = {
        type: "Paragraph",
        lines,
      };

      // Mark if this paragraph contains a subject_content (for document title detection)
      const firstLine = node.children.find((c) => c.tag === "text_line");
      if (firstLine) {
        const lc = firstLine.children.find((c) => c.tag === "line_content");
        if (lc) {
          const sc = lc.children.find((c) => c.tag === "subject_content");
          if (sc) {
            para._hasSubject = true;
            para._subjectText = extractText(sc).replace(/:$/, "");
          }
        }
      }

      return para;
    }

    case "annotation_block": {
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const label = headerNode ? extractText(headerNode).trim() : "";

      const blocks = convertBlockChildren({
        children: node.children.filter(
          (c) =>
            c.tag !== "annotation_marker" &&
            c.tag !== "annotation_header" &&
            c.tag !== "annotation_end_marker" &&
            c.tag !== "annotation_inline_text",
        ),
      });

      return {
        type: "Annotation",
        label,
        children: blocks.filter((b) => b !== null),
      };
    }

    case "annotation_single": {
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const label = headerNode ? extractText(headerNode).trim() : "";

      return {
        type: "Annotation",
        label,
        children: [],
      };
    }

    case "verbatim_block": {
      const subjectNode = findField(node, "subject");
      const subject = subjectNode ? extractSubject(subjectNode) : "";

      // Extract closing annotation label
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const closingLabel = headerNode ? extractText(headerNode).trim() : "";

      // Content lines — tree-sitter parses as blocks, lex treats as raw lines
      // For parity, we extract the content blocks
      const contentBlocks = convertBlockChildren({
        children: node.children.filter(
          (c) =>
            c.tag !== "annotation_marker" &&
            c.tag !== "annotation_header" &&
            !c.attrs.field,
        ),
      });

      return {
        type: "VerbatimBlock",
        closing_label: closingLabel,
        groups: [
          {
            subject,
            lines: contentBlocks.filter((b) => b !== null),
          },
        ],
      };
    }

    case "text_line": {
      return {
        type: "TextLine",
        content: extractText(node),
      };
    }

    default:
      return null;
  }
}

// Main
const input = require("fs").readFileSync("/dev/stdin", "utf8");
const root = parseXML(input);
const result = convertNode(root);
console.log(JSON.stringify(result, null, 2));
