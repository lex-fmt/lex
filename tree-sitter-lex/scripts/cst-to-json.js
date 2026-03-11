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

// Extract text content from a node.
// Only leaf nodes (no children) carry real text; non-leaf text is XML whitespace.
function extractText(node) {
  if (!node) return "";
  if (node.children.length === 0) return node.text || "";
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

// Convert consecutive blank_line nodes into BlankLineGroup
function groupBlanks(children) {
  const result = [];
  let blankCount = 0;

  for (const child of children) {
    if (child.tag === "blank_line") {
      blankCount++;
    } else {
      if (blankCount > 0) {
        result.push({ type: "BlankLineGroup", count: blankCount });
        blankCount = 0;
      }
      result.push(child);
    }
  }
  if (blankCount > 0) {
    result.push({ type: "BlankLineGroup", count: blankCount });
  }
  return result;
}

// Filter out blank_line nodes that are structural (session separators, etc.)
// and convert block children to canonical JSON
function convertBlockChildren(node) {
  const grouped = groupBlanks(node.children);
  return grouped
    .map((child) => {
      if (child.type === "BlankLineGroup") return child;
      return convertNode(child);
    })
    .filter((n) => n !== null);
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
      // Separate document-level annotations from content
      const annotations = [];
      const children = [];
      for (const block of blocks) {
        if (block && block.type === "Annotation" && children.length === 0) {
          annotations.push(block);
        } else if (block) {
          children.push(block);
        }
      }

      const doc = {
        type: "Document",
        title: "",
        children,
      };

      // Check if first child is a paragraph with subject_content (potential title)
      // The lex parser extracts document titles from first subject lines
      if (
        children.length > 0 &&
        children[0].type === "Paragraph" &&
        children[0]._hasSubject
      ) {
        doc.title = children[0]._subjectText || "";
      }

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
      const children = blocks.slice(startIdx).filter((b) => b !== null);

      const session = {
        type: "Session",
        title,
        children,
      };

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
