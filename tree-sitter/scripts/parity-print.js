#!/usr/bin/env node
/**
 * Print tree-sitter CST in parity format for comparison with `lex inspect parity`.
 *
 * Reads tree-sitter XML output from stdin and prints a plain-text block skeleton.
 * This script is intentionally minimal — it maps node types and extracts field
 * text, with zero assembly logic. If this script needs complex logic to make
 * tree-sitter output match lex-core, that's a real divergence to investigate.
 *
 * Usage:
 *   npx tree-sitter parse -x file.lex | node scripts/parity-print.js
 */

// --- Minimal XML parser (tree-sitter XML is simple and well-formed) ---

function parseXML(xml) {
  const nodes = [];
  const stack = [];
  let pos = 0;

  if (xml.startsWith("<?xml")) {
    pos = xml.indexOf("?>") + 2;
    while (pos < xml.length && xml[pos] === "\n") pos++;
  }

  while (pos < xml.length) {
    if (xml[pos] === "<") {
      if (xml[pos + 1] === "/") {
        const end = xml.indexOf(">", pos);
        const parent = stack.pop();
        if (stack.length > 0) {
          stack[stack.length - 1].children.push(parent);
        } else {
          nodes.push(parent);
        }
        pos = end + 1;
      } else {
        const end = xml.indexOf(">", pos);
        const tagContent = xml.substring(pos + 1, end);
        const selfClosing = tagContent.endsWith("/");
        const clean = selfClosing
          ? tagContent.slice(0, -1).trim()
          : tagContent.trim();

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
      const nextTag = xml.indexOf("<", pos);
      if (nextTag === -1) break;
      const text = xml.substring(pos, nextTag);
      if (stack.length > 0 && text.length > 0) {
        stack[stack.length - 1].text += text;
      }
      pos = nextTag;
    }
  }

  return nodes[0];
}

function decodeXML(text) {
  return text
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'");
}

// --- Text extraction helpers ---

function leafText(node) {
  if (!node) return "";
  if (node.children.length === 0) return decodeXML(node.text || "");
  let result = "";
  for (const child of node.children) {
    result += leafText(child);
  }
  return result;
}

function findField(node, fieldName) {
  for (const child of node.children) {
    if (child.attrs.field === fieldName) return child;
  }
  return null;
}

// --- Parity printer ---

function indent(depth) {
  return "  ".repeat(depth);
}

function printParity(node, depth) {
  if (!node || !node.tag) return;

  switch (node.tag) {
    case "document":
      console.log(`${indent(depth)}Document`);
      for (const child of node.children) {
        printParity(child, depth + 1);
      }
      break;

    case "document_title": {
      const titleNode = findField(node, "title");
      const title = titleNode ? leafText(titleNode) : "";
      console.log(`${indent(depth)}DocumentTitle "${title}"`);
      // Check for subtitle
      const subtitleNode = findField(node, "subtitle");
      if (subtitleNode) {
        const subtitle = leafText(subtitleNode);
        console.log(`${indent(depth + 1)}DocumentSubtitle "${subtitle}"`);
      }
      // Blank lines after title are structural separators, not semantic content —
      // lex-core AST doesn't include them in the DocumentTitle node

      break;
    }

    case "session": {
      const titleNode = findField(node, "title");
      const title = titleNode ? leafText(titleNode) : "";
      console.log(`${indent(depth)}Session "${title}"`);
      for (const child of node.children) {
        if (child.attrs.field === "title") continue; // already handled
        printParity(child, depth + 1);
      }
      break;
    }

    case "definition": {
      const subjectNode = findField(node, "subject");
      const subject = subjectNode ? leafText(subjectNode).replace(/:$/, "") : "";
      console.log(`${indent(depth)}Definition "${subject}"`);
      for (const child of node.children) {
        if (child.attrs.field === "subject") continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "list":
      console.log(`${indent(depth)}List`);
      for (const child of node.children) {
        printParity(child, depth + 1);
      }
      break;

    case "list_item": {
      const markerNode = node.children.find((c) => c.tag === "list_marker");
      const marker = markerNode ? leafText(markerNode).trimEnd() : "";
      console.log(`${indent(depth)}ListItem "${marker}"`);
      // Text content (first text_content child)
      const textNode = node.children.find((c) => c.tag === "text_content");
      if (textNode) {
        const text = leafText(textNode).trimStart();
        console.log(`${indent(depth + 1)}"${text}"`);
      }
      // Nested blocks
      for (const child of node.children) {
        if (child.tag === "list_marker" || child.tag === "text_content") continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "paragraph":
      console.log(`${indent(depth)}Paragraph`);
      for (const child of node.children) {
        printParity(child, depth + 1);
      }
      break;

    case "text_line": {
      const text = leafText(node).trimStart();
      console.log(`${indent(depth)}"${text}"`);
      break;
    }

    case "verbatim_block": {
      const subjectNode = findField(node, "subject");
      const subject = subjectNode ? leafText(subjectNode).replace(/:$/, "") : "";
      console.log(`${indent(depth)}VerbatimBlock "${subject}"`);
      for (const child of node.children) {
        if (child.attrs.field === "subject") continue;
        // Skip closing markers and annotation nodes
        if (
          child.tag === "annotation_marker" ||
          child.tag === "annotation_header" ||
          child.tag === "closing_label"
        )
          continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "verbatim_content": {
      // Verbatim content lines — extract raw text
      const text = leafText(node);
      if (text.trim()) {
        console.log(`${indent(depth)}"${text}"`);
      }
      break;
    }

    case "annotation_block": {
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const label = headerNode ? leafText(headerNode).trim() : "";
      console.log(`${indent(depth)}Annotation "${label}"`);
      for (const child of node.children) {
        if (
          child.tag === "annotation_marker" ||
          child.tag === "annotation_header" ||
          child.tag === "annotation_end_marker" ||
          child.tag === "annotation_inline_text"
        )
          continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "annotation_single": {
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const label = headerNode ? leafText(headerNode).trim() : "";
      console.log(`${indent(depth)}Annotation "${label}"`);
      break;
    }

    case "blank_line":
      console.log(`${indent(depth)}BlankLine`);
      break;

    default:
      // Skip structural/anonymous nodes, recurse into children
      for (const child of node.children) {
        printParity(child, depth);
      }
      break;
  }
}

// --- Main ---
const input = require("fs").readFileSync("/dev/stdin", "utf8");
const root = parseXML(input);
printParity(root, 0);
