export interface Action { id: string; title: string }
export type Color = 'red' | 'orange' | 'yellow' | 'green' | 'blue' | 'purple' | 'accent' | 'dim';
export interface Accessory { text?: string; tag?: string; icon?: string; color?: Color; tooltip?: string }
export interface MetadataRow { label?: string; text?: string; color?: Color; tags?: {text: string; color?: Color}[]; separator?: boolean }
export interface ItemDetail {
  hero?: {value: string; unit?: string; caption?: string; progress?: number; color?: Color};
  chart?: {values: number[]; caption?: string; color?: Color};
  text?: string;
  metadata?: MetadataRow[];
  /** Absolute path of a local image. */
  image?: string;
}
/** `icon` is a GNOME icon name or an absolute image path. */
export interface ListItem { id: string; title: string; subtitle?: string; icon?: string; tint?: Color; section?: string; progress?: number; accessories?: Accessory[]; detail?: ItemDetail; actions?: Action[] }
export interface ViewOptions { push?: boolean; context?: unknown; selection?: boolean }
export interface ListOptions extends ViewOptions { title?: string; placeholder?: string; detail?: boolean; refresh?: number; empty?: {title: string; description?: string; icon?: string} }
export interface GridItem { id: string; title: string; subtitle?: string; section?: string; tooltip?: string; content: {image?: string; icon?: string; tint?: Color}; actions?: Action[] }
export interface GridOptions extends ListOptions { columns?: number; aspect?: '1' | '3/2' | '2/3' | '4/3' | '3/4' | '16/9' | '9/16' }
export interface Field { id: string; label: string; type?: 'text' | 'password' | 'boolean' | 'dropdown' | 'description'; value?: string | boolean; placeholder?: string; required?: boolean; info?: string; text?: string; options?: {value: string; title: string}[] }
export type View = (({ type: 'list'; items: ListItem[] } & ListOptions) | ({ type: 'grid'; items: GridItem[] } & GridOptions) | ({ type: 'detail'; title: string; text: string; image?: string; metadata?: MetadataRow[] } & ViewOptions) | ({ type: 'form'; title: string; fields: Field[]; submit_label?: string } & ViewOptions));
export interface Context {
  command: string; query: string; signal: AbortSignal; item?: string; action?: string; values?: Record<string, string | boolean>;
  /** Merged extension and command preferences, with defaults applied. */
  preferences: Record<string, string | boolean>;
  /** Values typed into the command's arguments in the root search. */
  arguments: Record<string, string>;
  /** Data the command was opened with, for example from the top bar. */
  launch_context: unknown;
  /** The visible view's `context`, echoed back. */
  context?: unknown;
}
export interface Command { run(context: Context): View | null | void | Promise<View | null | void>; query?: Command['run']; action?: Command['run']; submit?: Command['run']; select?: Command['run'] }
export function serve(commands: Record<string, Command>): void;
export function update(view: View): void;
export function list(items: ListItem[], options?: string | ListOptions): View;
export function grid(items: GridItem[], options?: GridOptions): View;
export function push(view: View): View;
export function detail(title: string, text: string, options?: {image?: string; metadata?: MetadataRow[]}): View;
export function form(title: string, fields: Field[], submit_label?: string): View;
export const host: { copy(text: string): Promise<void>; open(url: string): Promise<void>; notify(text: string): Promise<void>; storage: {get<T = unknown>(key: string): Promise<T | null>; set(key: string, value: unknown): Promise<void>};
  toast(options: {style?: 'success' | 'failure' | 'animated'; title: string; message?: string}): Promise<void>;
  hud(text: string): Promise<void>; closeWindow(): Promise<void>; openPreferences(): Promise<void>;
  readClipboard(): Promise<{files?: string[]; image?: string; text?: string}>;
  copyFile(path: string): Promise<void>;
  chooseFiles(options?: {title?: string; mimeTypes?: string[]; multiple?: boolean; filterName?: string}): Promise<{files: string[]}> };
