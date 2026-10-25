export type Kind = "app" | "file" | "folder" | "command" | "calc";

export interface Hit {
  name: string;
  path: string;
  kind: Kind;
}
