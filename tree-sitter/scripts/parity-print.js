#!/usr/bin/env node
/**
 * Print tree-sitter CST in parity format for comparison with `lex inspect parity`.
 *
 * Reads tree-sitter XML output from stdin and prints a plain-text block skeleton.
 * This script is intentionally minimal — it maps node types and extracts field
 * text, with zero assembly logic. If this script needs complex logic to make
 * tree-sitter output match lex-core, that's a real divergence to investigate.
 *
 * The only non-trivial adaptation is stripping the verbatim indentation wall
 * from content lines, since tree-sitter reports raw source positions while
 * lex-core stores content relative to the wall. The wall column is read
 * directly from the CST node positions (paragraph scol inside verbatim_block).
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

function ind(depth) {
  return "  ".repeat(depth);
}

/**
 * Detect the verbatim wall column from a verbatim_block's content children.
 * The wall is the column where content starts (paragraph scol inside the block).
 * Returns -1 if no content found.
 */
function detectWallCol(verbatimNode) {
  for (const child of verbatimNode.children) {
    if (child.tag === "paragraph" || child.tag === "table_row") {
      return parseInt(child.attrs.scol || "0", 10);
    }
  }
  return -1;
}

/**
 * Strip wall indentation from a raw text line.
 * Tree-sitter reports continuation lines from character 0 (absolute),
 * while lex-core stores content relative to the wall.
 * scol is character-based (not column-based), so we strip exactly
 * wallCol characters of leading whitespace.
 */
function stripWall(text, wallCol, lineScol) {
  const col = parseInt(lineScol || "0", 10);
  if (col >= wallCol) {
    // Line starts at or past the wall — text is already wall-relative
    return text;
  }
  // Strip exactly wallCol characters of leading whitespace
  let i = 0;
  while (i < text.length && i < wallCol) {
    if (text[i] !== " " && text[i] !== "\t") break;
    i++;
  }
  return text.substring(i);
}

/**
 * Print nodes inside a verbatim_block. Tree-sitter wraps verbatim content
 * in paragraph > text_line nodes; lex-core has flat VerbatimLine nodes.
 * We flatten paragraph/text_line to raw quoted lines to match.
 */
function printVerbatimChild(node, depth, wallCol) {
  switch (node.tag) {
    case "paragraph":
      // Flatten: don't print Paragraph, just print its text lines
      for (const child of node.children) {
        printVerbatimChild(child, depth, wallCol);
      }
      break;

    case "text_line": {
      // Get the line's source column to determine wall-stripping
      const lineScol = node.attrs.scol || "0";
      const rawText = leafText(node);
      const text = stripWall(rawText, wallCol, lineScol);
      console.log(`${ind(depth)}"${text}"`);
      break;
    }

    case "blank_line":
      // Inside verbatim, blank lines become empty VerbatimLines
      console.log(`${ind(depth)}""`);
      break;

    case "table_row": {
      // Reconstruct pipe-delimited line from cells, trimming cell padding
      const cells = node.children
        .filter((c) => c.tag === "table_cell")
        .map((c) => leafText(c).trim());
      const line = `| ${cells.join(" | ")} |`;
      console.log(`${ind(depth)}"${line}"`);
      break;
    }

    case "separator_line": {
      const lineScol = node.attrs.scol || "0";
      const rawText = leafText(node);
      const text = stripWall(rawText, wallCol, lineScol);
      console.log(`${ind(depth)}"${text}"`);
      break;
    }

    default:
      // Skip annotation markers, closing labels, etc.
      break;
  }
}

function printParity(node, depth) {
  if (!node || !node.tag) return;

  switch (node.tag) {
    case "document": {
      console.log(`${ind(depth)}Document`);
      const children = node.children;
      for (let i = 0; i < children.length; i++) {
        const child = children[i];
        // Suppress trailing blank line at document level — CST artifact from final newline;
        // lex-core AST does not track trailing document blank lines
        if (i === children.length - 1 && child.tag === "blank_line") continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "document_title": {
      const titleNode = findField(node, "title");
      const subtitleNode = node.children.find(
        (c) => c.tag === "document_subtitle",
      );
      let title = titleNode ? leafText(titleNode) : "";
      // When subtitle is present, the trailing colon on the title is structural
      // (delimiter between title and subtitle) — lex-core strips it
      if (subtitleNode) {
        title = title.replace(/:$/, "");
      }
      console.log(`${ind(depth)}DocumentTitle "${title}"`);
      if (subtitleNode) {
        const subtitle = leafText(subtitleNode);
        console.log(`${ind(depth + 1)}DocumentSubtitle "${subtitle}"`);
      }
      // Blank lines after title are structural separators, not semantic —
      // lex-core AST doesn't include them in DocumentTitle
      break;
    }

    case "session": {
      const titleNode = findField(node, "title");
      const title = titleNode ? leafText(titleNode) : "";
      console.log(`${ind(depth)}Session "${title}"`);
      for (const child of node.children) {
        if (child.attrs.field === "title") continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "definition": {
      const subjectNode = findField(node, "subject");
      const subject = subjectNode
        ? leafText(subjectNode).replace(/:$/, "")
        : "";
      console.log(`${ind(depth)}Definition "${subject}"`);
      for (const child of node.children) {
        if (child.attrs.field === "subject") continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "list":
      console.log(`${ind(depth)}List`);
      for (const child of node.children) {
        printParity(child, depth + 1);
      }
      break;

    case "list_item": {
      const markerNode = node.children.find((c) => c.tag === "list_marker");
      const marker = markerNode ? leafText(markerNode).trimEnd() : "";
      console.log(`${ind(depth)}ListItem "${marker}"`);
      const textNode = node.children.find((c) => c.tag === "text_content");
      if (textNode) {
        const text = leafText(textNode).trimStart();
        console.log(`${ind(depth + 1)}"${text}"`);
      }
      for (const child of node.children) {
        if (child.tag === "list_marker" || child.tag === "text_content")
          continue;
        printParity(child, depth + 1);
      }
      break;
    }

    case "paragraph":
      console.log(`${ind(depth)}Paragraph`);
      for (const child of node.children) {
        printParity(child, depth + 1);
      }
      break;

    case "text_line": {
      const text = leafText(node).trimStart().trimEnd();
      console.log(`${ind(depth)}"${text}"`);
      break;
    }

    case "verbatim_block": {
      const subjectNode = findField(node, "subject");
      const subject = subjectNode
        ? leafText(subjectNode).replace(/:$/, "")
        : "";
      console.log(`${ind(depth)}VerbatimBlock "${subject}"`);
      const wallCol = detectWallCol(node);
      for (const child of node.children) {
        if (child.attrs.field === "subject") continue;
        if (
          child.tag === "annotation_marker" ||
          child.tag === "annotation_header"
        )
          continue;
        printVerbatimChild(child, depth + 1, wallCol);
      }
      break;
    }

    case "annotation_block": {
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      // annotation_header contains "label params..." — extract just the label (first word).
      // Strip trailing colon from label (in ":: label: value ::", the colon is a separator)
      const headerText = headerNode ? leafText(headerNode).trim() : "";
      const label = (headerText.split(/\s+/)[0] || "").replace(/:$/, "");
      console.log(`${ind(depth)}Annotation "${label}"`);
      for (const child of node.children) {
        if (
          child.tag === "annotation_marker" ||
          child.tag === "annotation_header" ||
          child.tag === "annotation_end_marker"
        )
          continue;
        // Annotation inline text becomes a Paragraph child in lex-core.
        // The space between closing :: and text is part of the content in lex-core.
        if (child.tag === "annotation_inline_text") {
          const inlineText = leafText(child);
          if (inlineText) {
            console.log(`${ind(depth + 1)}Paragraph`);
            console.log(`${ind(depth + 2)}" ${inlineText}"`);
          }
          continue;
        }
        printParity(child, depth + 1);
      }
      break;
    }

    case "annotation_single": {
      const headerNode = node.children.find(
        (c) => c.tag === "annotation_header",
      );
      const headerText = headerNode ? leafText(headerNode).trim() : "";
      const label = (headerText.split(/\s+/)[0] || "").replace(/:$/, "");
      console.log(`${ind(depth)}Annotation "${label}"`);
      // Inline text in single-line annotations
      const inlineNode = node.children.find(
        (c) => c.tag === "annotation_inline_text",
      );
      if (inlineNode) {
        const inlineText = leafText(inlineNode);
        if (inlineText) {
          console.log(`${ind(depth + 1)}Paragraph`);
          console.log(`${ind(depth + 2)}" ${inlineText}"`);
        }
      }
      break;
    }

    case "blank_line":
      console.log(`${ind(depth)}BlankLine`);
      break;

    default:
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
