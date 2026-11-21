/** Helpers shared by the webview scripts. Everything dynamic goes through `textContent`: never HTML. */

declare function acquireVsCodeApi(): { postMessage(message: unknown): void };

const api = acquireVsCodeApi();

export function post(message: unknown): void {
  api.postMessage(message);
}

type Child = Node | string | null | undefined;

export interface Props {
  readonly class?: string;
  readonly text?: string;
  readonly title?: string;
  readonly type?: string;
  readonly placeholder?: string;
  readonly tabIndex?: number;
  readonly onclick?: (event: MouseEvent) => void;
  readonly onkeydown?: (event: KeyboardEvent) => void;
}

/** Builds an element. Strings are appended as text nodes, so model names cannot inject markup. */
export function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Props = {}, ...children: Child[]): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (props.class !== undefined) {
    element.className = props.class;
  }
  if (props.text !== undefined) {
    element.textContent = props.text;
  }
  if (props.title !== undefined) {
    element.title = props.title;
  }
  if (props.type !== undefined) {
    element.setAttribute('type', props.type);
  }
  if (props.placeholder !== undefined && element instanceof HTMLInputElement) {
    element.placeholder = props.placeholder;
  }
  if (props.tabIndex !== undefined) {
    element.tabIndex = props.tabIndex;
  }
  if (props.onclick !== undefined) {
    element.addEventListener('click', props.onclick as EventListener);
  }
  if (props.onkeydown !== undefined) {
    element.addEventListener('keydown', props.onkeydown as EventListener);
  }
  for (const child of children) {
    if (child !== null && child !== undefined) {
      element.append(child);
    }
  }
  return element;
}

export function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`missing element #${id}`);
  }
  return element as T;
}

export function clear(element: Element): void {
  element.replaceChildren();
}

/** Makes a table row act as a button: click, Enter or Space. */
export function activatable(row: HTMLElement, label: string, action: () => void): void {
  row.classList.add('link');
  row.tabIndex = 0;
  row.setAttribute('role', 'button');
  row.setAttribute('aria-label', label);
  row.addEventListener('click', action);
  row.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      action();
    }
  });
}

/** Handles `{ type: 'error' | 'busy' }` and passes the rest on. */
export function onHostMessage(handlers: {
  data: (message: Record<string, unknown>) => void;
  error?: (text: string) => void;
  busy?: (on: boolean) => void;
}): void {
  window.addEventListener('message', (event: MessageEvent<unknown>) => {
    const message = event.data;
    if (typeof message !== 'object' || message === null) {
      return;
    }
    const record = message as Record<string, unknown>;
    if (record.type === 'error' && typeof record.message === 'string') {
      handlers.error?.(record.message);
    } else if (record.type === 'busy') {
      handlers.busy?.(record.on === true);
    } else if (record.type === 'data') {
      handlers.data(record);
    }
  });
}

export function themeColour(name: string, fallback: string): string {
  const value = getComputedStyle(document.body).getPropertyValue(name).trim();
  return value.length > 0 ? value : fallback;
}
