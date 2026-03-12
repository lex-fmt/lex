#!/usr/bin/env node
/**
 * CST-to-JSON bridge: converts tree-sitter XML output to a JSON
 * representation of the CST structure.
 *
 * This is a THIN type mapper — it translates tree-sitter node types to
 * canonical JSON names but performs NO assembly logic (no annotation
 * attachment, no verbatim group merging, no title detection, no BLG
 * manipulation). The output reflects what tree-sitter actually parsed.
 *
 * Usage:
 *   npx tree-sitter parse -x file.lex | node scripts/cst-to-json.js
 *   node scripts/cst-to-json.js < cst.xml
 */

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
  return text.replace(/:$/, "");
}

// Extract title text from a session's title field
function extractTitle(node) {
  const titleNode = findField(node, "title");
  if (!titleNode) return "";
  return extractText(titleNode);
}

// Extract list marker from list_item_line text (e.g., "- First item" → "-")
function extractListMarker(text) {
  if (text.startsWith("- ")) return "-";

  const parenMatch = text.match(/^\(([^)]+)\)\s/);
  if (parenMatch) return `(${parenMatch[1]})`;

  const orderedMatch = text.match(/^([0-9a-zA-Z.]+[.)]) /);
  if (orderedMatch) return orderedMatch[1];

  return "";
}

// Extract list item text (everything after marker)
function extractListItemText(text) {
  if (text.startsWith("- ")) return text.substring(2);

  const parenMatch = text.match(/^\([^)]+\)\s(.*)/);
  if (parenMatch) return parenMatch[1];

  const orderedMatch = text.match(/^[0-9a-zA-Z.]+[.)]\s(.*)/);
  if (orderedMatch) return orderedMatch[1];

  return text;
}

// Convert block children: map blank_lines to BlankLineGroups, convert others
function convertBlockChildren(node) {
  return node.children
    .map((child) => {
      if (child.tag === "blank_line") {
        return { type: "BlankLineGroup", count: 1 };
      }
      return convertNode(child);
    })
    .filter((n) => n !== null);
}

// Convert a single CST node to canonical JSON
function convertNode(node) {
  if (!node || !node.tag) return null;

  switch (node.tag) {
    case "document": {
      const children = convertBlockChildren(node);
      return {
        type: "Document",
        children,
      };
    }

    case "session": {
      const title = extractTitle(node);
      const children = convertBlockChildren(node).filter((b) => b !== null);
      return {
        type: "Session",
        title,
        children,
      };
    }

    case "definition": {
      const subjectNode = findField(node, "subject");
      const subject = subjectNode ? extractSubject(subjectNode) : "";
      const children = convertBlockChildren(node).filter((b) => b !== null);
      return {
        type: "Definition",
        subject,
        children,
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
          const content = extractText(tl).replace(/^[ \t]+/, "");
          return { type: "TextLine", content };
        });
      return {
        type: "Paragraph",
        lines,
      };
    }

    case "annotation_block": {
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const label = headerNode ? extractText(headerNode).trim() : "";

      const children = convertBlockChildren({
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
        children: children.filter((b) => b !== null),
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

      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const closingLabel = headerNode ? extractText(headerNode).trim() : "";

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
