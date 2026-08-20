import { useId, useMemo, useState, type FormEvent } from "react";

import { Icon } from "./Icon";

type AdvancedField =
  | "rating"
  | "size"
  | "width"
  | "height"
  | "aspect"
  | "created"
  | "modified"
  | "duration"
  | "pages"
  | "orientation"
  | "root"
  | "path"
  | "color-space"
  | "has-note"
  | "has-alpha";

type ComparisonOperator = "=" | "<" | "<=" | ">" | ">=";
type BuilderOperator = ComparisonOperator | "unknown";
type FieldKind =
  | "integer"
  | "size"
  | "duration"
  | "ratio"
  | "instant"
  | "orientation"
  | "root"
  | "path"
  | "color-space"
  | "boolean"
  | "nullable-boolean";

interface FieldDefinition {
  field: AdvancedField;
  label: string;
  kind: FieldKind;
  defaultOperator: BuilderOperator;
  defaultValue: string;
  supportsUnknown?: boolean;
}

export interface AdvancedPredicateDraft {
  field: AdvancedField;
  operator: BuilderOperator;
  value: string;
  unit?: string;
}

export type PredicateBuildResult =
  { ok: true; predicate: string } | { ok: false; message: string };

const comparisonOperators: readonly ComparisonOperator[] = [
  "=",
  "<",
  "<=",
  ">",
  ">=",
];

const fieldDefinitions: readonly FieldDefinition[] = [
  {
    field: "rating",
    label: "评分",
    kind: "integer",
    defaultOperator: ">=",
    defaultValue: "4",
  },
  {
    field: "size",
    label: "文件大小",
    kind: "size",
    defaultOperator: ">=",
    defaultValue: "10",
    supportsUnknown: true,
  },
  {
    field: "width",
    label: "宽度",
    kind: "integer",
    defaultOperator: ">=",
    defaultValue: "1920",
    supportsUnknown: true,
  },
  {
    field: "height",
    label: "高度",
    kind: "integer",
    defaultOperator: ">=",
    defaultValue: "1080",
    supportsUnknown: true,
  },
  {
    field: "aspect",
    label: "宽高比",
    kind: "ratio",
    defaultOperator: ">=",
    defaultValue: "16/9",
    supportsUnknown: true,
  },
  {
    field: "created",
    label: "创建时间",
    kind: "instant",
    defaultOperator: ">=",
    defaultValue: "2026-01-01T00:00:00Z",
    supportsUnknown: true,
  },
  {
    field: "modified",
    label: "修改时间",
    kind: "instant",
    defaultOperator: ">=",
    defaultValue: "2026-01-01T00:00:00Z",
    supportsUnknown: true,
  },
  {
    field: "duration",
    label: "时长",
    kind: "duration",
    defaultOperator: ">=",
    defaultValue: "30",
    supportsUnknown: true,
  },
  {
    field: "pages",
    label: "页数",
    kind: "integer",
    defaultOperator: ">=",
    defaultValue: "2",
    supportsUnknown: true,
  },
  {
    field: "orientation",
    label: "方向",
    kind: "orientation",
    defaultOperator: "=",
    defaultValue: "landscape",
  },
  {
    field: "root",
    label: "素材根",
    kind: "root",
    defaultOperator: "=",
    defaultValue: "",
    supportsUnknown: true,
  },
  {
    field: "path",
    label: "相对路径包含",
    kind: "path",
    defaultOperator: "=",
    defaultValue: "",
  },
  {
    field: "color-space",
    label: "颜色空间",
    kind: "color-space",
    defaultOperator: "=",
    defaultValue: "srgb",
    supportsUnknown: true,
  },
  {
    field: "has-note",
    label: "有备注",
    kind: "boolean",
    defaultOperator: "=",
    defaultValue: "true",
  },
  {
    field: "has-alpha",
    label: "Alpha 通道",
    kind: "nullable-boolean",
    defaultOperator: "=",
    defaultValue: "true",
  },
];

const definitionsByField = new Map(
  fieldDefinitions.map((definition) => [definition.field, definition]),
);

const sizeUnits = ["B", "KiB", "MiB", "GiB", "TiB"] as const;
const durationUnits = ["ms", "s", "min", "h"] as const;
const maximumU64 = 18_446_744_073_709_551_615n;

export function AdvancedFilterBuilder({
  onAdd,
}: {
  onAdd: (predicate: string) => void;
}) {
  const fieldId = useId();
  const operatorId = useId();
  const valueId = useId();
  const unitId = useId();
  const errorId = useId();
  const [open, setOpen] = useState(false);
  const [field, setField] = useState<AdvancedField>("rating");
  const [operator, setOperator] = useState<BuilderOperator>(">=");
  const [value, setValue] = useState("4");
  const [unit, setUnit] = useState("");
  const definition = definitionsByField.get(field)!;
  const result = useMemo(
    () => buildAdvancedPredicate({ field, operator, value, unit }),
    [field, operator, unit, value],
  );

  const selectField = (nextField: AdvancedField) => {
    const next = definitionsByField.get(nextField)!;
    setField(nextField);
    setOperator(next.defaultOperator);
    setValue(next.defaultValue);
    setUnit(next.kind === "size" ? "MiB" : next.kind === "duration" ? "s" : "");
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!result.ok) return;
    onAdd(result.predicate);
  };

  return (
    <div className="advanced-filter-builder">
      <button
        aria-controls="advanced-filter-panel"
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label="添加高级属性条件"
        className="query-builder-toggle"
        onClick={() => setOpen((current) => !current)}
        type="button"
      >
        <Icon name="plus" size={14} />
        <span>条件</span>
      </button>
      {open ? (
        <form
          aria-label="高级属性条件编辑器"
          className="advanced-filter-panel"
          id="advanced-filter-panel"
          onSubmit={submit}
        >
          <div className="advanced-filter-heading">
            <strong>添加属性条件</strong>
            <button
              aria-label="关闭高级属性条件编辑器"
              className="advanced-filter-close"
              onClick={() => setOpen(false)}
              type="button"
            >
              <Icon name="close" size={14} />
            </button>
          </div>
          <div className="advanced-filter-controls">
            <label htmlFor={fieldId}>
              字段
              <select
                id={fieldId}
                onChange={(event) =>
                  selectField(event.target.value as AdvancedField)
                }
                value={field}
              >
                {fieldDefinitions.map((item) => (
                  <option key={item.field} value={item.field}>
                    {item.label}
                  </option>
                ))}
              </select>
            </label>
            <label htmlFor={operatorId}>
              操作符
              <select
                id={operatorId}
                onChange={(event) =>
                  setOperator(event.target.value as BuilderOperator)
                }
                value={operator}
              >
                {operatorsFor(definition).map((item) => (
                  <option key={item} value={item}>
                    {item === "unknown" ? "未知" : item}
                  </option>
                ))}
              </select>
            </label>
            {operator !== "unknown" ? (
              <label className="advanced-filter-value" htmlFor={valueId}>
                值
                {valueOptions(definition.kind) ? (
                  <select
                    aria-describedby={!result.ok ? errorId : undefined}
                    aria-invalid={!result.ok}
                    id={valueId}
                    onChange={(event) => setValue(event.target.value)}
                    value={value}
                  >
                    {valueOptions(definition.kind)!.map((item) => (
                      <option key={item.value} value={item.value}>
                        {item.label}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    aria-describedby={!result.ok ? errorId : undefined}
                    aria-invalid={!result.ok}
                    id={valueId}
                    onChange={(event) => setValue(event.target.value)}
                    placeholder={valuePlaceholder(definition.kind)}
                    spellCheck={false}
                    value={value}
                  />
                )}
              </label>
            ) : null}
            {operator !== "unknown" && definition.kind === "size" ? (
              <UnitSelect
                id={unitId}
                label="大小单位"
                onChange={setUnit}
                options={sizeUnits}
                value={unit || "MiB"}
              />
            ) : null}
            {operator !== "unknown" && definition.kind === "duration" ? (
              <UnitSelect
                id={unitId}
                label="时长单位"
                onChange={setUnit}
                options={durationUnits}
                value={unit || "s"}
              />
            ) : null}
          </div>
          <div className="advanced-filter-footer">
            <span className="advanced-filter-error" id={errorId} role="status">
              {result.ok ? `将添加：${result.predicate}` : result.message}
            </span>
            <button disabled={!result.ok} type="submit">
              添加条件
            </button>
          </div>
        </form>
      ) : null}
    </div>
  );
}

function UnitSelect({
  id,
  label,
  onChange,
  options,
  value,
}: {
  id: string;
  label: string;
  onChange: (value: string) => void;
  options: readonly string[];
  value: string;
}) {
  return (
    <label htmlFor={id}>
      {label}
      <select
        id={id}
        onChange={(event) => onChange(event.target.value)}
        value={value}
      >
        {options.map((option) => (
          <option key={option}>{option}</option>
        ))}
      </select>
    </label>
  );
}

export function buildAdvancedPredicate(
  draft: AdvancedPredicateDraft,
): PredicateBuildResult {
  const definition = definitionsByField.get(draft.field);
  if (!definition) return invalid("请选择受支持的字段");
  if (!operatorsFor(definition).includes(draft.operator)) {
    return invalid("该字段不支持所选操作符");
  }
  if (draft.operator === "unknown") {
    return { ok: true, predicate: `${draft.field}:unknown` };
  }

  const value = draft.value.trim();
  const validation = validateValue(definition, value, draft.unit ?? "");
  if (!validation.ok) return validation;
  const operator = draft.operator === "=" ? "" : draft.operator;
  return {
    ok: true,
    predicate: `${draft.field}:${operator}${validation.value}`,
  };
}

export function appendAdvancedPredicate(
  expression: string,
  predicate: string,
): string {
  return [expression.trim(), predicate].filter(Boolean).join(" ");
}

function operatorsFor(definition: FieldDefinition): readonly BuilderOperator[] {
  const comparison = [
    ...comparisonOperators,
    ...(definition.supportsUnknown ? (["unknown"] as const) : []),
  ];
  if (
    [
      "orientation",
      "root",
      "path",
      "color-space",
      "boolean",
      "nullable-boolean",
    ].includes(definition.kind)
  ) {
    return definition.supportsUnknown ? ["=", "unknown"] : ["="];
  }
  return comparison;
}

function validateValue(
  definition: FieldDefinition,
  value: string,
  unit: string,
): { ok: true; value: string } | { ok: false; message: string } {
  if (definition.kind === "integer") {
    if (!/^\d+$/u.test(value)) return invalid("值必须是十进制整数");
    const parsed = BigInt(value);
    if (parsed > maximumU64) return invalid("整数超出支持范围");
    if (definition.field === "rating" && parsed > 5n) {
      return invalid("评分必须在 0 到 5 之间");
    }
    if (
      ["width", "height", "pages"].includes(definition.field) &&
      parsed === 0n
    ) {
      return invalid("该字段必须大于 0");
    }
    return { ok: true, value };
  }
  if (definition.kind === "size" || definition.kind === "duration") {
    if (!/^\d+$/u.test(value)) return invalid("值必须是非负十进制整数");
    const units = definition.kind === "size" ? sizeUnits : durationUnits;
    if (!units.includes(unit as never)) return invalid("请选择受支持的单位");
    const multiplier = unitMultiplier(unit);
    if (BigInt(value) * multiplier > maximumU64) {
      return invalid("换算后的整数超出支持范围");
    }
    return { ok: true, value: `${value}${unit}` };
  }
  if (definition.kind === "ratio") {
    const match = /^(\d+)\/(\d+)$/u.exec(value);
    if (!match) return invalid("宽高比必须使用正整数分数，例如 16/9");
    const numerator = Number(match[1]);
    const denominator = Number(match[2]);
    if (
      numerator < 1 ||
      numerator > 1_000_000 ||
      denominator < 1 ||
      denominator > 1_000_000
    ) {
      return invalid("分子和分母必须在 1 到 1000000 之间");
    }
    return { ok: true, value };
  }
  if (definition.kind === "instant") {
    const rfc3339 =
      /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/u;
    if (!rfc3339.test(value) || !Number.isFinite(Date.parse(value))) {
      return invalid("时间必须是带 Z 或时区偏移的 RFC 3339 值");
    }
    return { ok: true, value };
  }
  if (definition.kind === "orientation") {
    return ["landscape", "portrait", "square", "unknown"].includes(value)
      ? { ok: true, value }
      : invalid("请选择受支持的方向");
  }
  if (definition.kind === "root") {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(
      value,
    )
      ? { ok: true, value }
      : invalid("素材根必须是规范小写 UUID");
  }
  if (definition.kind === "path") {
    const invalidSegment = value
      .split("/")
      .some((part) => part === "." || part === "..");
    if (
      !value ||
      value.startsWith("/") ||
      value.includes("\\") ||
      /^[A-Za-z]:/u.test(value) ||
      invalidSegment ||
      /\p{Cc}/u.test(value)
    ) {
      return invalid("路径必须是无点段和反斜杠的根内相对子串");
    }
    return { ok: true, value: quoteValue(value.normalize("NFC")) };
  }
  if (definition.kind === "color-space") {
    return value.length <= 64 && /^[a-z0-9][a-z0-9._-]*$/u.test(value)
      ? { ok: true, value }
      : invalid("颜色空间必须是规范小写提供器值");
  }
  if (definition.kind === "boolean" || definition.kind === "nullable-boolean") {
    const allowed =
      definition.kind === "nullable-boolean"
        ? ["true", "false", "unknown"]
        : ["true", "false"];
    return allowed.includes(value)
      ? { ok: true, value }
      : invalid("请选择受支持的布尔值");
  }
  return invalid("无法生成该条件");
}

function valueOptions(
  kind: FieldKind,
): readonly { value: string; label: string }[] | undefined {
  if (kind === "orientation") {
    return [
      { value: "landscape", label: "横向" },
      { value: "portrait", label: "纵向" },
      { value: "square", label: "正方形" },
      { value: "unknown", label: "未知" },
    ];
  }
  if (kind === "boolean") {
    return [
      { value: "true", label: "是" },
      { value: "false", label: "否" },
    ];
  }
  if (kind === "nullable-boolean") {
    return [
      { value: "true", label: "是" },
      { value: "false", label: "否" },
      { value: "unknown", label: "未知" },
    ];
  }
  return undefined;
}

function valuePlaceholder(kind: FieldKind): string {
  if (kind === "ratio") return "16/9";
  if (kind === "instant") return "2026-01-01T00:00:00Z";
  if (kind === "root") return "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx";
  if (kind === "path") return "Brand Assets/icons";
  if (kind === "color-space") return "display-p3";
  return "0";
}

function unitMultiplier(unit: string): bigint {
  const multipliers: Readonly<Record<string, bigint>> = {
    B: 1n,
    KiB: 1024n,
    MiB: 1024n ** 2n,
    GiB: 1024n ** 3n,
    TiB: 1024n ** 4n,
    ms: 1n,
    s: 1_000n,
    min: 60_000n,
    h: 3_600_000n,
  };
  return multipliers[unit] ?? 0n;
}

function quoteValue(value: string): string {
  return `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

function invalid(message: string): { ok: false; message: string } {
  return { ok: false, message };
}
