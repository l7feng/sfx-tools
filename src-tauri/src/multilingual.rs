//! 本地中/英/日音效词表扩展。纯查表、无联网、无模型。
//!
//! 只把「普通文本关键词」扩展成同一概念的三语候选（词内 OR），
//! 词与词之间仍是 AND；标签/分类/格式/时长/收藏/播放列表等筛选条件完全不参与，
//! 所有候选词仍走 SQL 绑定参数，不拼进 SQL 文本。
//! 未登录词原样返回，等于没有启用跨语言时的行为。

/// 单个词最多扩展成几个候选，防止把 SQL 参数数量放大到不可控。
pub const MAX_VARIANTS: usize = 6;
/// 一次查询最多接受多少个扩展候选（含各词原词）。
pub const MAX_TOTAL_VARIANTS: usize = 64;

/// 每行是一组同义/近义的音效概念：中文、英文、日文常见写法混排。
/// 词条只用于「文件名 / 路径 / 标签」的字面子串匹配，不做分词、不做语义推理。
static GROUPS: &[&[&str]] = &[
    // 天气与自然
    &["雨", "rain", "rainy", "降雨", "雨声", "雨音"],
    &["雷", "thunder", "雷鳴", "thunderstorm", "雷声"],
    &["风", "wind", "風", "windy", "强风"],
    &["雪", "snow", "雪", "snowfall"],
    &["水", "water", "水", "水流", "みず"],
    &["海", "sea", "ocean", "海", "wave", "波", "波浪"],
    &["火", "fire", "flame", "炎", "火焰", "火事"],
    &["水滴", "drip", "drop", "水滴", "水滴音"],
    &["夜晚", "night", "夜", "夜间", "nighttime"],
    &["早晨", "morning", "朝", "清晨"],
    &["夏天", "summer", "夏"],
    &["冬天", "winter", "冬"],
    &["森林", "forest", "森", "woods", "树林"],
    &["城市", "city", "都市", "urban", "街", "城区"],
    // 动物
    &["鸟", "bird", "鳥", "小鸟", "小鳥", "chirp"],
    &["狗", "dog", "犬", "bark", "吠"],
    &["猫", "cat", "猫", "meow", "喵"],
    &["虫", "insect", "虫", "bug", "蝉"],
    &["马", "horse", "馬", "hoof"],
    // 人与动作
    &["脚步", "footstep", "footsteps", "足音", "歩行", "步行"],
    &["人声", "voice", "声", "vocal", "话音"],
    &["笑声", "laugh", "笑い", "laughter", "笑"],
    &["哭", "cry", "泣き", "crying", "呜咽"],
    &["人群", "crowd", "群衆", "people", "人混み"],
    &["掌声", "applause", "拍手", "clap", "鼓掌"],
    &["呼吸", "breath", "息", "breathing"],
    &["心跳", "heartbeat", "心音", "heart"],
    &["咳嗽", "cough", "咳"],
    &["婴儿", "baby", "赤ちゃん", "婴儿哭"],
    &["口哨", "whistle", "笛", "口笛"],
    // 物件与机械
    &["门", "door", "ドア", "扉", "开门", "关门"],
    &["车", "car", "車", "vehicle", "汽车"],
    &["飞机", "airplane", "plane", "飛行機", "飞行"],
    &["火车", "train", "電車", "列车"],
    &["引擎", "engine", "エンジン", "motor", "发动机"],
    &["机器", "machine", "機械", "mechanical"],
    &["电话", "phone", "電話", "ring", "铃声"],
    &["时钟", "clock", "時計", "tick", "钟表"],
    &["铃", "bell", "鈴", "ベル", "风铃", "風鈴", "chime"],
    &["玻璃", "glass", "ガラス", "碎玻璃"],
    &["金属", "metal", "金属", "metallic"],
    &["木头", "wood", "木", "wooden"],
    &["纸", "paper", "紙", "rustle"],
    &["开关", "switch", "スイッチ", "toggle"],
    &["警报", "alarm", "警報", "siren", "サイレン"],
    &["喇叭", "horn", "クラクション", "car horn"],
    // 冲击与拟音
    &["爆炸", "explosion", "爆発", "blast", "boom", "爆破"],
    &["撞击", "impact", "衝撃", "hit", "crash", "碰撞"],
    &["破碎", "break", "割れ", "smash", "shatter", "碎裂"],
    &["枪", "gun", "銃", "shot", "gunshot", "枪声"],
    &["转场", "transition", "転換", "whoosh", "swoosh", "呼啸"],
    &["上升", "rise", "上昇", "riser", "上升音"],
    &["下降", "fall", "下降", "downlifter", "下落"],
    &["回声", "echo", "エコー", "reverb", "残响"],
    &["噪音", "noise", "ノイズ", "white noise", "白噪声"],
    // 音乐与情绪
    &["音乐", "music", "音楽", "bgm", "配乐"],
    &["鼓", "drum", "ドラム", "percussion", "打击乐"],
    &["钢琴", "piano", "ピアノ"],
    &["吉他", "guitar", "ギター"],
    &["低音", "bass", "低音", "sub", "subbass"],
    &["恐怖", "horror", "ホラー", "scary", "惊悚"],
    &["快乐", "happy", "幸せ", "明るい", "欢快"],
    &["悲伤", "sad", "悲しい", "哀伤"],
    &["紧张", "tension", "緊張", "suspense", "悬疑"],
    &["环境", "ambience", "ambient", "環境音", "氛围"],
    &["循环", "loop", "ループ", "循环音"],
    &["科幻", "scifi", "sci-fi", "SF", "サイエンス"],
    &["魔法", "magic", "魔法", "magical"],
    &["战争", "war", "戦争", "battle", "战斗"],
];

/// 把一个查询词扩展成三语候选。返回的第一项恒为原词（ASCII 转小写），
/// 保证「关掉扩展」与「没命中词表」时的匹配语义完全一致。
pub fn expand(word: &str) -> Vec<String> {
    let original = word.to_ascii_lowercase();
    let mut out = vec![original.clone()];
    for group in GROUPS {
        let hit = group.iter().any(|item| {
            item.eq_ignore_ascii_case(word) || item.eq_ignore_ascii_case(&original)
        });
        if !hit { continue; }
        for item in *group {
            if out.len() >= MAX_VARIANTS { break; }
            let value = item.to_ascii_lowercase();
            if !out.contains(&value) { out.push(value); }
        }
        break;
    }
    out
}

/// 整条查询的扩展结果：词间 AND 由调用方用括号分组实现。
/// 超限时退回「只保留原词」，宁可少召回也不放大 SQL。
pub fn expand_query(words: &[&str]) -> Vec<Vec<String>> {
    let mut total = 0usize;
    let mut result = Vec::with_capacity(words.len());
    for word in words {
        let variants = expand(word);
        total += variants.len();
        result.push(variants);
    }
    if total > MAX_TOTAL_VARIANTS {
        return words.iter().map(|w| vec![w.to_ascii_lowercase()]).collect();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_expands_to_english_and_japanese() {
        let v = expand("雨");
        assert_eq!(v[0], "雨");
        assert!(v.contains(&"rain".to_string()));
        assert!(v.contains(&"雨音".to_string()));
    }

    #[test]
    fn english_expands_to_chinese() {
        let v = expand("RAIN");
        assert_eq!(v[0], "rain");
        assert!(v.contains(&"雨".to_string()));
    }

    #[test]
    fn japanese_kana_expands_to_english() {
        let v = expand("足音");
        assert!(v.contains(&"footstep".to_string()));
        assert!(v.contains(&"脚步".to_string()));
    }

    #[test]
    fn unknown_word_is_unchanged() {
        assert_eq!(expand("_100%"), vec!["_100%"]);
        assert_eq!(expand("needle"), vec!["needle"]);
    }

    #[test]
    fn variants_are_bounded_and_unique() {
        for group in GROUPS {
            for word in *group {
                let v = expand(word);
                assert!(v.len() <= MAX_VARIANTS);
                assert_eq!(v[0], word.to_ascii_lowercase());
                let mut sorted = v.clone();
                sorted.sort();
                sorted.dedup();
                assert_eq!(sorted.len(), v.len(), "候选词不应重复：{word}");
            }
        }
    }

    #[test]
    fn oversized_query_falls_back_to_literal_words() {
        let words: Vec<&str> = (0..40).map(|_| "雨").collect();
        let expanded = expand_query(&words);
        assert_eq!(expanded.len(), 40);
        assert!(expanded.iter().all(|v| v.len() == 1));
    }

    #[test]
    fn sql_control_characters_are_never_injected_into_text() {
        // 词表里不含引号、分号、注释符；注入串不会命中任何词条。
        let v = expand("' OR 1=1 --");
        assert_eq!(v, vec!["' or 1=1 --"]);
        for group in GROUPS {
            for item in *group {
                assert!(!item.contains('\'') && !item.contains(';'), "词条含危险字符：{item}");
            }
        }
    }
}
