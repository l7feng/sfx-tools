import React from 'react';
import type {ToolPage} from './tool-navigation';

/**
 * 工具页图标。统一 24×24 线性几何，stroke 走 currentColor，
 * 供二级标签栏（13px）与工具总览卡片（14px）复用同一套形状。
 */
const PATHS:Record<ToolPage,React.ReactNode>={
  // UCS 分类建议：标签
  ucs:<><path d="M4.2 6.6A2.4 2.4 0 0 1 6.6 4.2h4a1.7 1.7 0 0 1 1.2.5l7.1 7.1a1.7 1.7 0 0 1 0 2.4l-4.7 4.7a1.7 1.7 0 0 1-2.4 0l-7.1-7.1a1.7 1.7 0 0 1-.5-1.2z"/><circle cx="8.6" cy="8.6" r="1.2"/></>,
  // 重复素材：两个错位矩形
  duplicates:<><rect x="3.8" y="3.8" width="10.6" height="10.6" rx="2.2"/><path d="M9.8 20.2h8.4a2 2 0 0 0 2-2V9.8"/></>,
  // 批量导出与转换：双向箭头（重采样／变速）
  convert:<><path d="M4 8.2h11.4"/><path d="M12.4 5.2 15.6 8.2l-3.2 3"/><path d="M20 15.8H8.6"/><path d="M11.6 12.8 8.4 15.8l3.2 3"/></>,
  // 插件与制作连接：插头
  plugins:<><path d="M9 3.6v3.2"/><path d="M15 3.6v3.2"/><path d="M6.6 6.8h10.8v4.6a5.4 5.4 0 0 1-10.8 0z"/><path d="M12 16.8v3.6"/></>,
  // 叠层编排：三层堆叠
  layer:<><path d="M12 3.4 20.6 7.8 12 12.2 3.4 7.8z"/><path d="M3.4 12.4 12 16.8l8.6-4.4"/><path d="M3.4 16.6 12 21l8.6-4.4"/></>,
  // Pro Tools 导入预览：向下导入托盘
  pt:<><path d="M12 3.6v9.2"/><path d="M8.4 9.4 12 13l3.6-3.6"/><path d="M4.2 15.4v3.2a1.8 1.8 0 0 0 1.8 1.8h12a1.8 1.8 0 0 0 1.8-1.8v-3.2"/></>,
  // 账号与授权：人像
  account:<><circle cx="12" cy="8.2" r="3.7"/><path d="M4.9 20.4c0-3.6 3.2-6.1 7.1-6.1s7.1 2.5 7.1 6.1"/></>,
  // 素材库维护：健康曲线（检查失联）
  maintenance:<><path d="M3.4 12.4h3.8l2.3-4.6 3.1 8.2 2.4-3.6h5.6"/></>,
  // 音频信息：信息圈
  info:<><circle cx="12" cy="12" r="8.3"/><path d="M12 11.2v5.4"/><circle cx="12" cy="8.1" r=".95" fill="currentColor" stroke="none"/></>,
  // UCS 批量命名：清单 + 改写笔
  'ucs-namer':<><path d="M4 5.6h8.4"/><path d="M4 12h5.6"/><path d="M4 18.4h4.2"/><path d="m15.4 8.4 3.2 3.2-5 5-3.7.5.5-3.7z"/></>,
  // MUS Prompt：对话气泡
  'mus-prompt':<><path d="M4.2 5.6h15.6v9.6H9.8L5.4 19.4v-4.2H4.2z"/><path d="M8 9.4h8"/><path d="M8 12.4h5"/></>,
};

export function ToolGlyph({id}:{id:ToolPage}){
 return <svg className="tool-glyph-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{PATHS[id]}</svg>;
}

/** 工具页顶栏的通用动作图标：总览 / 返回。 */
const UI_PATHS={
 grid:<><rect x="3.8" y="3.8" width="6.6" height="6.6" rx="1.6"/><rect x="13.6" y="3.8" width="6.6" height="6.6" rx="1.6"/><rect x="3.8" y="13.6" width="6.6" height="6.6" rx="1.6"/><rect x="13.6" y="13.6" width="6.6" height="6.6" rx="1.6"/></>,
 back:<><path d="M10.4 5.6 4 12l6.4 6.4"/><path d="M4.6 12h15"/></>,
} as const;

export function UiGlyph({name}:{name:keyof typeof UI_PATHS}){
 return <svg className="tool-glyph-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{UI_PATHS[name]}</svg>;
}
