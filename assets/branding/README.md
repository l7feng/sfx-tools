# SFX TOOLS 图标

2026-11-09（0.1.25）按人类裁决改为「蓝色声波柱」并全端统一：深色圆角方块 + 五根蓝色渐变声波柱。同一几何同时用于 `src/brand-mark.tsx`（顶部 SVG）、`assets/icons/app.png`、`assets/icons/app.ico`（EXE、窗口与快捷方式同一枚母版）。图标由脚本按 SVG 几何程序化渲染（Pillow，1024px，ICO 含 16–256px），不再依赖 AI 出图，保证小尺寸与头部标志完全一致。

历史：2026-10-28 首版为暖金声波（imagegen 生成），母版提示词存档如下，仅追溯用，已被蓝色版取代。

首稿中线容易产生货币符号联想，已改成纯横向声波/声波柱，无字样。没有使用原 SoundSeeker 图标。

最终生成提示：One polished professional audio workstation Windows app icon, square 1024x1024. Deep charcoal matte rounded-square tile, transparent corners. Center a bold warm champagne gold emblem: a continuous smooth horizontal audio waveform folded into an abstract sideways S, with two broad curved lobes and generous dark negative space; asymmetric waveform sweeps left to right. Crisp minimal silhouette readable at 32px, restrained satin metal tonal depth. NO vertical line through the emblem, NO dollar sign, NO currency symbol, NO treble clef, no letters or words, no headphones, no musical note, no equalizer bars, no thin detailing, no glow, no neon. Premium studio equipment aesthetic, understated, technical, balanced margins. A final single app icon, not a mockup or grid.
