import type {
  CharacterStyle,
  LayerDetails,
  LayerPage,
  LayerResponse,
  LayerSummary,
  ParagraphStyle,
  TextSlice,
} from './generated';

const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null;
const uint = (v: unknown): v is number =>
  typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= 4294967295;
const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const bool = (v: unknown): v is boolean => typeof v === 'boolean';
const string = (v: unknown): v is string => typeof v === 'string';
const nullableString = (v: unknown) => v === null || string(v);
const id = (v: unknown) =>
  string(v) && /^[1-9][0-9]{0,19}$/.test(v) && BigInt(v) <= 18446744073709551615n;
const capability = (v: unknown) =>
  record(v) &&
  ['supported', 'partial', 'unsupported'].includes(String(v.status)) &&
  string(v.reason);
const metric = (v: unknown) => record(v) && finite(v.value) && v.unit === 'unverifiedEngine';
const font = (v: unknown) =>
  record(v) &&
  uint(v.index) &&
  string(v.name) &&
  nullableString(v.family) &&
  nullableString(v.style);
const color = (v: unknown) =>
  record(v) &&
  ['red', 'green', 'blue', 'alpha'].every((key) => finite(v[key]) && v[key] >= 0 && v[key] <= 1);
const alignment = (v: unknown) =>
  string(v) &&
  [
    'left',
    'right',
    'center',
    'justifyLeft',
    'justifyRight',
    'justifyCenter',
    'justifyAll',
  ].includes(v);
const property = (v: unknown, check: (v: unknown) => boolean) =>
  v === null || (record(v) && string(v.source) && check(v.value));
const characterFields = {
  font,
  fontSize: metric,
  fillColor: color,
  fillEnabled: bool,
  strokeEnabled: bool,
  fauxBold: bool,
  fauxItalic: bool,
  autoLeading: bool,
  leading: metric,
  tracking: metric,
  baselineShift: metric,
  horizontalScale: finite,
  verticalScale: finite,
} satisfies Record<keyof CharacterStyle, (v: unknown) => boolean>;
const paragraphFields = {
  alignment,
  autoLeading: finite,
  firstLineIndent: metric,
  startIndent: metric,
  endIndent: metric,
  spaceBefore: metric,
  spaceAfter: metric,
} satisfies Record<keyof ParagraphStyle, (v: unknown) => boolean>;
const style = (v: unknown, fields: Record<string, (v: unknown) => boolean>) =>
  record(v) && Object.entries(fields).every(([key, check]) => property(v[key], check));

export function isLayerSummary(v: unknown): v is LayerSummary {
  return (
    record(v) &&
    uint(v.id) &&
    (v.parentId === null || (uint(v.parentId) && v.parentId < v.id)) &&
    string(v.name) &&
    bool(v.nameTruncated) &&
    ['group', 'bitmap', 'text', 'shape', 'smartObject', 'adjustment', 'unknown'].includes(
      String(v.kind),
    ) &&
    record(v.bounds) &&
    finite(v.bounds.x) &&
    Number.isInteger(v.bounds.x) &&
    v.bounds.x >= -2147483648 &&
    v.bounds.x <= 2147483647 &&
    finite(v.bounds.y) &&
    Number.isInteger(v.bounds.y) &&
    v.bounds.y >= -2147483648 &&
    v.bounds.y <= 2147483647 &&
    uint(v.bounds.width) &&
    uint(v.bounds.height) &&
    bool(v.visible) &&
    bool(v.effectiveVisible) &&
    uint(v.opacity) &&
    v.opacity <= 255
  );
}
function page(v: unknown): v is LayerPage {
  if (
    !record(v) ||
    !uint(v.offset) ||
    !uint(v.total) ||
    v.total > 4096 ||
    !Array.isArray(v.layers) ||
    v.layers.length > 128 ||
    !v.layers.every(isLayerSummary)
  )
    return false;
  const end = v.offset + v.layers.length;
  const offset = v.offset;
  return (
    end <= v.total &&
    (v.nextOffset === null
      ? end === v.total
      : v.nextOffset === end && end > v.offset && end < v.total) &&
    v.layers.every((layer, index) => layer.id === offset + index)
  );
}
function textSlice(v: unknown): v is TextSlice {
  if (
    !record(v) ||
    !string(v.text) ||
    !uint(v.start) ||
    !uint(v.end) ||
    !uint(v.totalLength) ||
    v.start > v.end ||
    v.end > v.totalLength ||
    v.text.length !== v.end - v.start ||
    v.text.length > 2048 ||
    (v.nextStart === null
      ? v.end !== v.totalLength
      : v.nextStart !== v.end || v.end <= v.start || v.end >= v.totalLength) ||
    !(
      v.transform === null ||
      (Array.isArray(v.transform) && v.transform.length === 6 && v.transform.every(finite))
    ) ||
    !capability(v.styles) ||
    !(
      v.indexMapping === null ||
      v.indexMapping === 'exact' ||
      v.indexMapping === 'trailingParagraphTerminator'
    ) ||
    !bool(v.diagnosticsTruncated) ||
    !Array.isArray(v.diagnostics) ||
    v.diagnostics.length > 32 ||
    !v.diagnostics.every(
      (d) =>
        record(d) &&
        [
          'missing',
          'invalid',
          'unsupported',
          'unverified',
          'textMismatch',
          'invalidRange',
        ].includes(String(d.code)) &&
        string(d.path) &&
        string(d.message),
    )
  )
    return false;
  const start = v.start,
    end = v.end,
    total = v.totalLength;
  const runs = (value: unknown, fields: Record<string, (v: unknown) => boolean>) =>
    value === null ||
    (Array.isArray(value) &&
      value.length <= 128 &&
      value.every(
        (run, index) =>
          record(run) &&
          uint(run.start) &&
          uint(run.end) &&
          run.start < run.end &&
          run.end <= total &&
          run.end > start &&
          run.start < end &&
          (index === 0 || (record(value[index - 1]) && value[index - 1].end === run.start)) &&
          style(run.style, fields),
      ));
  return runs(v.styleRuns, characterFields) && runs(v.paragraphRuns, paragraphFields);
}
export function isLayerDetails(v: unknown): v is LayerDetails {
  return (
    record(v) &&
    isLayerSummary(v.layer) &&
    capability(v.export) &&
    Array.isArray(v.exportBlockers) &&
    v.exportBlockers.every((b) =>
      [
        'unsupportedLayerKind',
        'hidden',
        'globalMask',
        'ancestorVisualDependency',
        'layerVisualDependency',
        'missingRgbChannels',
        'emptyBitmap',
      ].includes(b),
    ) &&
    Array.isArray(v.diagnostics) &&
    v.diagnostics.length <= 32 &&
    v.diagnostics.every(string) &&
    bool(v.diagnosticsTruncated) &&
    (v.text === null || textSlice(v.text))
  );
}
/** 校验有界图层、文字区间及所有实际读取的样式字段；生成类型不能代替运行时校验。 */
export function isLayerResponse(v: unknown): v is LayerResponse {
  return (
    record(v) &&
    id(v.documentId) &&
    id(v.revision) &&
    record(v.result) &&
    (v.result.kind === 'list'
      ? page(v.result.page)
      : v.result.kind === 'details' && isLayerDetails(v.result.details))
  );
}
