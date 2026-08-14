import { readFile } from 'node:fs/promises';

import {
  MarkdownRenderChild,
  Notice,
  Plugin,
  PluginSettingTab,
  Setting,
  type App,
} from 'obsidian';

import { buildAssetIndex } from './indexer';
import { parseMaterialUri } from './reference';
import type { AssetLocation } from './types';

interface MaterialBridgeSettings {
  roots: string[];
}

const DEFAULT_SETTINGS: MaterialBridgeSettings = { roots: [] };
const MAX_PROTOTYPE_FILE_SIZE = 25 * 1024 * 1024;

export default class MaterialBridgePlugin extends Plugin {
  settings: MaterialBridgeSettings = DEFAULT_SETTINGS;
  private assets = new Map<string, AssetLocation>();

  async onload(): Promise<void> {
    this.settings = Object.assign({}, DEFAULT_SETTINGS, await this.loadData());
    this.addSettingTab(new MaterialBridgeSettingTab(this.app, this));
    this.addCommand({
      id: 'rebuild-material-index',
      name: 'Rebuild material index',
      callback: async () => {
        await this.rebuildIndex(true);
      },
    });
    this.registerMarkdownPostProcessor((element, context) => {
      for (const image of element.querySelectorAll('img')) {
        const id = parseMaterialUri(image.getAttribute('src') ?? '');
        if (id === undefined) {
          continue;
        }
        const location = this.assets.get(id);
        if (location === undefined) {
          image.replaceWith(createMissingElement(id));
          continue;
        }
        const child = new ExternalImageRenderChild(image, location);
        context.addChild(child);
        void child.render();
      }
    });
    await this.rebuildIndex(false);
  }

  async rebuildIndex(showNotice: boolean): Promise<void> {
    const result = await buildAssetIndex(this.settings.roots);
    this.assets = result.assets;
    if (showNotice) {
      new Notice(`Indexed ${result.assets.size} materials; ${result.problems.length} problems`);
    }
  }

  async saveSettings(): Promise<void> {
    await this.saveData(this.settings);
    await this.rebuildIndex(false);
  }
}

class ExternalImageRenderChild extends MarkdownRenderChild {
  private objectUrl: string | undefined;

  constructor(
    private readonly image: HTMLImageElement,
    private readonly location: AssetLocation,
  ) {
    super(image);
  }

  async render(): Promise<void> {
    const bytes = await readFile(this.location.assetPath);
    if (bytes.byteLength > MAX_PROTOTYPE_FILE_SIZE) {
      this.image.replaceWith(createErrorElement('Material exceeds the 25 MiB prototype limit'));
      return;
    }
    const content = new Uint8Array(bytes.byteLength);
    content.set(bytes);
    this.objectUrl = URL.createObjectURL(new Blob([content], { type: this.location.mime }));
    this.image.src = this.objectUrl;
  }

  onunload(): void {
    if (this.objectUrl !== undefined) {
      URL.revokeObjectURL(this.objectUrl);
      this.objectUrl = undefined;
    }
  }
}

class MaterialBridgeSettingTab extends PluginSettingTab {
  constructor(app: App, private readonly plugin: MaterialBridgePlugin) {
    super(app, plugin);
  }

  display(): void {
    this.containerEl.empty();
    new Setting(this.containerEl)
      .setName('Authorized material roots')
      .setDesc('One absolute directory per line. Symbolic links are not indexed.')
      .addTextArea((text) => {
        text
          .setValue(this.plugin.settings.roots.join('\n'))
          .onChange(async (value) => {
            this.plugin.settings.roots = value
              .split('\n')
              .map((root) => root.trim())
              .filter((root) => root.length > 0);
            await this.plugin.saveSettings();
          });
      });
  }
}

function createMissingElement(id: string): HTMLElement {
  const element = document.createElement('span');
  element.textContent = `Missing material: ${id}`;
  element.className = 'material-bridge-error';
  return element;
}

function createErrorElement(message: string): HTMLElement {
  const element = document.createElement('span');
  element.textContent = message;
  element.className = 'material-bridge-error';
  return element;
}
