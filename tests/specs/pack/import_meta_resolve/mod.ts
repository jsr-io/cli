export { x } from "./helper.ts";

export function helperUrl(): string {
  return import.meta.resolve("./helper.ts");
}
