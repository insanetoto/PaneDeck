export interface DirectoryReference {
  sessionId: string;
}

export interface EntryReference extends DirectoryReference {
  entryId: string;
}

export type EntryKind = "file" | "directory" | "symlink" | "other";
export type SymlinkTarget = "file" | "directory" | "other" | "missing" | "unknown";

export interface EntrySnapshot {
  reference: EntryReference;
  displayName: string;
  displayNameIsLossy: boolean;
  kind: EntryKind;
  byteLen: string | null;
  modifiedAtUnixMillis: string | null;
  isHidden: boolean;
  isReadOnly: boolean;
  symlinkTarget: SymlinkTarget | null;
}
