import { invoke } from '@tauri-apps/api/core';
import { IPC_PROTOCOL_VERSION } from '../shared/api/generated';
import type { RenderScene, RenderLayer, RenderPath, TextData, RenderProfile } from './generated';

const object = (v: unknown): v is Record<string, unknown> =>
  !!v && typeof v === 'object' && !Array.isArray(v);
const finite = (v: unknown): v is number =>
  typeof v === 'number' && Number.isFinite(v) && Math.abs(v) <= 100_000;
const integer = (v: unknown): v is number => finite(v) && Number.isInteger(v) && v >= 0;
const bytes = (v: unknown) =>
  Array.isArray(v) && v.length <= 4 * 1024 * 1024 && v.every((n) => integer(n) && n <= 255);
const property = (v: unknown, check: (x: unknown) => boolean) =>
  v === null || (object(v) && typeof v.source === 'string' && check(v.value));
const metric = (v: unknown) => object(v) && finite(v.value) && v.unit === 'unverifiedEngine';
const color = (v: unknown) =>
  object(v) &&
  ['red', 'green', 'blue', 'alpha'].every((k) => finite(v[k]) && v[k] >= 0 && v[k] <= 1);
const textBox = (v: unknown) =>
  v === null ||
  (object(v) &&
    finite(v.x) &&
    finite(v.y) &&
    finite(v.width) &&
    v.width > 0 &&
    finite(v.height) &&
    v.height > 0);
function text(v: unknown): v is TextData {
  return (
    object(v) &&
    typeof v.rawText === 'string' &&
    v.rawText.length <= 4096 &&
    v.utf16Length === v.rawText.length &&
    (v.transform === null ||
      (Array.isArray(v.transform) && v.transform.length === 6 && v.transform.every(finite))) &&
    (v.paragraphRuns === null ||
      (Array.isArray(v.paragraphRuns) &&
        v.paragraphRuns.length <= 64 &&
        v.paragraphRuns.every((r) => {
          if (
            !object(r) ||
            !integer(r.start) ||
            !integer(r.end) ||
            r.end < r.start ||
            r.end > (v.rawText as string).length ||
            !object(r.style)
          )
            return false;
          const style = r.style;
          return (
            property(
              style.alignment,
              (a) =>
                typeof a === 'string' &&
                [
                  'left',
                  'right',
                  'center',
                  'justifyLeft',
                  'justifyRight',
                  'justifyCenter',
                  'justifyAll',
                ].includes(a),
            ) &&
            property(style.autoLeading, finite) &&
            ['firstLineIndent', 'startIndent', 'endIndent', 'spaceBefore', 'spaceAfter'].every(
              (k) => property(style[k], metric),
            )
          );
        }))) &&
    (v.styleRuns === null ||
      (Array.isArray(v.styleRuns) &&
        v.styleRuns.length <= 64 &&
        v.styleRuns.every((r) => {
          if (
            !object(r) ||
            !integer(r.start) ||
            !integer(r.end) ||
            r.end < r.start ||
            r.end > (v.rawText as string).length ||
            !object(r.style)
          )
            return false;
          const s = r.style;
          return (
            property(s.font, (f) => object(f) && typeof f.name === 'string') &&
            ['fontSize', 'leading', 'tracking', 'baselineShift'].every((k) =>
              property(s[k], metric),
            ) &&
            property(s.fillColor, color) &&
            ['fillEnabled', 'strokeEnabled', 'fauxBold', 'fauxItalic', 'autoLeading'].every((k) =>
              property(s[k], (b) => typeof b === 'boolean'),
            ) &&
            ['horizontalScale', 'verticalScale'].every((k) => property(s[k], finite))
          );
        })))
  );
}
function path(v: unknown): v is RenderPath {
  return (
    object(v) &&
    typeof v.evenOdd === 'boolean' &&
    Array.isArray(v.knots) &&
    v.knots.length <= 256 &&
    v.knots.every((k) => Array.isArray(k) && k.length === 6 && k.every(finite))
  );
}
function layer(v: unknown): v is RenderLayer {
  if (
    !object(v) ||
    !integer(v.id) ||
    !(v.parentId === null || integer(v.parentId)) ||
    typeof v.name !== 'string' ||
    v.name.length > 1024 ||
    !object(v.bounds) ||
    !finite(v.bounds.x) ||
    !finite(v.bounds.y) ||
    !integer(v.bounds.width) ||
    !integer(v.bounds.height) ||
    typeof v.visible !== 'boolean' ||
    !integer(v.opacity) ||
    v.opacity > 255 ||
    !object(v.content) ||
    !(
      v.compositionWarning === null ||
      (typeof v.compositionWarning === 'string' && v.compositionWarning.length <= 512)
    )
  )
    return false;
  const c = v.content;
  switch (c.kind) {
    case 'group':
      return true;
    case 'unsupported':
      return typeof c.reason === 'string';
    case 'bitmap':
      return bytes(c.png);
    case 'text':
      return text(c.data) && textBox(c.textBox);
    case 'path':
      return (
        Array.isArray(c.paths) &&
        c.paths.length <= 32 &&
        c.paths.every(path) &&
        Array.isArray(c.color) &&
        c.color.length === 4 &&
        c.color.every((n) => finite(n) && n >= 0 && n <= 1)
      );
    default:
      return false;
  }
}
export function isRenderScene(v: unknown, profile: RenderProfile = 'simple'): v is RenderScene {
  return (
    object(v) &&
    typeof v.sourceSha256 === 'string' &&
    /^[0-9a-f]{64}$/.test(v.sourceSha256) &&
    integer(v.width) &&
    v.width > 0 &&
    integer(v.height) &&
    v.height > 0 &&
    v.width * v.height <=
      (profile === 'simple'
        ? 512 * 512
        : profile === 'bounded'
          ? 16 * 1024 * 1024
          : 32 * 1024 * 1024) &&
    Array.isArray(v.layers) &&
    v.layers.length <= (profile === 'simple' ? 64 : 2048) &&
    v.layers.every(layer) &&
    v.layers.reduce((sum, l) => sum + l.bounds.width * l.bounds.height, v.width * v.height) <=
      (profile === 'simple'
        ? 512 * 512 * 2
        : profile === 'bounded'
          ? 32 * 1024 * 1024
          : 96 * 1024 * 1024) &&
    new Set(v.layers.map((l) => l.id)).size === v.layers.length &&
    bytes(v.referencePng) &&
    Array.isArray(v.warnings) &&
    v.warnings.every((w) => typeof w === 'string')
  );
}
const boot = window.__LAYERLENS_DEV_BOOT__;
export const largeExperiment = object(boot) && boot.largeRender === true;
export async function readScene(
  documentId: string,
  revision: string,
  profile: RenderProfile = 'bounded',
): Promise<RenderScene> {
  const value: unknown = await invoke('read_render_scene', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION, documentId, revision },
    profile,
  });
  if (!isRenderScene(value, profile)) throw new Error('实验场景超出契约或预算');
  return value;
}

/** 与清单绑定同一修订；仅大稿实验二进制入口，单项不超过 32 MiB。 */
export async function readResource(
  documentId: string,
  revision: string,
  profile: RenderProfile,
  layerId: number | null,
): Promise<ArrayBuffer> {
  const value: unknown = await invoke('read_render_resource', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION, documentId, revision },
    layerId,
    profile,
  });
  if (
    !(value instanceof ArrayBuffer) ||
    value.byteLength === 0 ||
    value.byteLength > 32 * 1024 * 1024
  )
    throw new Error('实验二进制资源超出契约');
  return value;
}

/** PSD 控制点转 SVG 路径，几何已在 Rust 中规范化为文档像素。 */
export function pathData(path: RenderPath): string {
  const first = path.knots[0];
  if (!first) return '';
  let result = 'M ' + first[2] + ' ' + first[3];
  for (let i = 1; i <= path.knots.length; i++) {
    const previous = path.knots[i - 1],
      current = path.knots[i % path.knots.length];
    if (previous && current)
      result +=
        ' C ' +
        [previous[4], previous[5], current[0], current[1], current[2], current[3]].join(' ');
  }
  return result + ' Z';
}
export const cssColor = (c: readonly number[]) =>
  'rgba(' +
  c
    .slice(0, 3)
    .map((n) => Math.round(n * 255))
    .join(',') +
  ',' +
  (c[3] ?? 1) +
  ')';
