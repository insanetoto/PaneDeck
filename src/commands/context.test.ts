import { describe, expect, it } from "vitest";
import type { DirectoryListing } from "../browser/directoryApi";
import { captureCommandContext, isTextInput } from "./context";

const listing = (sessionId: string): DirectoryListing => ({
  directory: { sessionId },
  displayPath: `/${sessionId}`,
  canGoUp: true,
  warnings: [],
  entries: [
    {
      reference: { sessionId, entryId: "1" },
      displayName: "file",
      displayNameIsLossy: false,
      kind: "file",
      byteLen: "1",
      modifiedAtUnixMillis: null,
      isHidden: false,
      isReadOnly: false,
      symlinkTarget: null,
    },
  ],
});

describe("command context", () => {
  it("preserves captured source and destination when live state changes", () => {
    const listings = { left: listing("left"), right: listing("right") };
    const context = captureCommandContext("contextMenu", "left", listings, listings.left.entries);
    listings.left.entries[0].reference.entryId = "changed";
    listings.right.directory.sessionId = "changed";
    expect(context.entries[0].reference).toEqual({ sessionId: "left", entryId: "1" });
    expect(context.target).toEqual({
      kind: "directory",
      pane: "right",
      directory: { sessionId: "right" },
    });
  });
  it("uses the clicked pane root and no stale selection for blank menus", () => {
    const listings = { left: listing("left"), right: listing("right") };
    const context = captureCommandContext("blankMenu", "right", listings, listings.left.entries);
    expect(context.entries).toEqual([]);
    expect(context.directory).toEqual({ sessionId: "right" });
  });
  it("captures a drag destination entry rather than the current pane root", () => {
    const listings = { left: listing("left"), right: listing("right") };
    const context = captureCommandContext("drag", "left", listings, listings.left.entries, {
      kind: "entry",
      pane: "right",
      entry: { sessionId: "right", entryId: "folder" },
    });
    expect(context.target?.kind).toBe("entry");
  });
  it("protects nested editable controls without suppressing file-list shortcuts", () => {
    const editor = document.createElement("div");
    editor.contentEditable = "true";
    editor.setAttribute("contenteditable", "true");
    const child = document.createElement("span");
    editor.append(child);
    expect(isTextInput(child)).toBe(true);
    expect(isTextInput(document.createElement("input"))).toBe(true);
    expect(isTextInput(document.createElement("div"))).toBe(false);
  });
});
