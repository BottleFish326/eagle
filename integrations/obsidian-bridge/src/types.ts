export interface AssetSidecar {
  schema: 1;
  id: string;
  tags: string[];
  rating?: number;
  favorite?: boolean;
  note?: string;
  aliases?: string[];
  updatedAt: string;
  [key: string]: unknown;
}

export interface AssetLocation {
  id: string;
  rootPath: string;
  assetPath: string;
  sidecarPath: string;
  mime: string;
  tags: string[];
}

export interface IndexProblem {
  path: string;
  message: string;
}

export interface IndexResult {
  assets: Map<string, AssetLocation>;
  problems: IndexProblem[];
}
