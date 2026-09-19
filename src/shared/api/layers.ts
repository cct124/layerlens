import { invoke } from '@tauri-apps/api/core';
import { IPC_PROTOCOL_VERSION } from './generated';
import type { LayerQuery, LayerResponse } from './generated';
import { isLayerResponse } from './layerValidation';

/** 图层查询固定文档修订；调用方串行调度并丢弃已经失去展示目标的响应。 */
export async function readLayers(
  documentId: string,
  revision: string,
  query: LayerQuery,
): Promise<LayerResponse> {
  const response: unknown = await invoke('read_layers', {
    request: { protocolVersion: IPC_PROTOCOL_VERSION, documentId, revision, query },
  });
  if (
    !isLayerResponse(response) ||
    response.documentId !== documentId ||
    response.revision !== revision
  )
    throw new Error('图层响应无效或修订不匹配，请重新读取。');
  if (query.kind === 'list') {
    if (
      response.result.kind !== 'list' ||
      response.result.page.offset !== query.offset ||
      response.result.page.layers.length > query.limit
    )
      throw new Error('图层分页响应与请求不匹配。');
  } else if (
    response.result.kind !== 'details' ||
    response.result.details.layer.id !== query.layerId ||
    (response.result.details.text && response.result.details.text.start !== query.textStart)
  ) {
    throw new Error('图层属性响应与请求不匹配。');
  }
  return response;
}
