//! ASCII 兼容工具: file_contexts 的 ASCII 强制与中文音译。
//!
//! Android file_contexts 要求纯 ASCII。本模块提供:
//! - `ensure_ascii`: 校验字符串是否纯 ASCII, 非法时返回错误。
//! - `transliterate`: 将非 ASCII (主要是中文) 转换为 ASCII 近似表示。
//!
//! 中文音译使用 pinyin 映射表 (常用字符的拼音, 无声调)。

/// 校验字符串是否为纯 ASCII。
pub fn is_ascii(s: &str) -> bool {
    s.is_ascii()
}

/// 校验字符串是否为纯 ASCII, 非法时返回错误信息。
pub fn ensure_ascii(s: &str, context: &str) -> anyhow::Result<()> {
    if is_ascii(s) {
        Ok(())
    } else {
        let non_ascii: Vec<char> = s.chars().filter(|c| !c.is_ascii()).collect();
        Err(anyhow::anyhow!(
            "{context} contains non-ASCII characters: {}",
            non_ascii.iter().collect::<String>()
        ))
    }
}

/// 将路径中的非 ASCII 字符音译为 ASCII (主要是中文 → 拼音)。
///
/// 策略:
/// 1. 常见中文字符 → 拼音 (无声调, 小写)
/// 2. 其他非 ASCII 字符 → 下划线
/// 3. ASCII 字符保持不变
pub fn transliterate_path(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii() {
            result.push(ch);
        } else if let Some(pinyin) = pinyin_of(ch) {
            result.push_str(pinyin);
        } else {
            result.push('_');
        }
    }
    result
}

/// 将 SELinux 上下文音译为 ASCII。
/// 上下文格式: `user:role:type:mls`, 只音译非 ASCII 部分。
pub fn transliterate_context(ctx: &str) -> String {
    if is_ascii(ctx) {
        return ctx.to_string();
    }
    // 逐字符处理, 保持冒号和 ASCII 部分不变
    let mut result = String::with_capacity(ctx.len());
    for ch in ctx.chars() {
        if ch.is_ascii() {
            result.push(ch);
        } else if let Some(pinyin) = pinyin_of(ch) {
            result.push_str(pinyin);
        } else {
            result.push('_');
        }
    }
    result
}

/// 查询中文字符的拼音 (无声调)。
/// 覆盖常用字符, 未覆盖的返回 None (调用方回退为下划线)。
fn pinyin_of(ch: char) -> Option<&'static str> {
    PINYIN_TABLE.iter().find(|(c, _)| *c == ch).map(|(_, p)| *p)
}

/// 常用中文字符 → 拼音映射表 (无声调)。
/// 仅覆盖 Android 系统镜像中可能出现的文件/目录名用字。
static PINYIN_TABLE: &[(char, &str)] = &[
    // 常用字
    ('\u{4e2d}', "zhong"), ('\u{56fd}', "guo"), ('\u{6587}', "wen"), ('\u{4ef6}', "jian"),
    ('\u{76ee}', "mu"), ('\u{5f55}', "lu"), ('\u{6570}', "shu"), ('\u{636e}', "ju"),
    ('\u{7cfb}', "xi"), ('\u{7edf}', "tong"), ('\u{8bbe}', "she"), ('\u{5907}', "bei"),
    ('\u{7528}', "yong"), ('\u{6237}', "hu"), ('\u{5b89}', "an"), ('\u{5353}', "zhuo"),
    ('\u{5e94}', "ying"), ('\u{7a0b}', "cheng"), ('\u{5e8f}', "xu"), ('\u{5305}', "bao"),
    ('\u{540d}', "ming"), ('\u{5b57}', "zi"), ('\u{7b26}', "fu"), ('\u{4e32}', "chuan"),
    ('\u{914d}', "pei"), ('\u{7f6e}', "zhi"), ('\u{6587}', "wen"), ('\u{672c}', "ben"),
    ('\u{56fe}', "tu"), ('\u{7247}', "pian"), ('\u{97f3}', "yin"), ('\u{9891}', "pin"),
    ('\u{89c6}', "shi"), ('\u{9891}', "pin"), ('\u{4e0b}', "xia"), ('\u{8f7d}', "zai"),
    ('\u{4e0a}', "shang"), ('\u{4f20}', "chuan"), ('\u{7f13}', "huan"), ('\u{5b58}', "cun"),
    ('\u{65e5}', "ri"), ('\u{5fd7}', "zhi"), ('\u{4e34}', "lin"), ('\u{65f6}', "shi"),
    ('\u{9ed8}', "mo"), ('\u{8ba4}', "ren"), ('\u{9519}', "cuo"), ('\u{8bef}', "wu"),
    ('\u{8b66}', "jing"), ('\u{544a}', "gao"), ('\u{6743}', "quan"), ('\u{9650}', "xian"),
    ('\u{5bc6}', "mi"), ('\u{94a5}', "yue"), ('\u{8bc1}', "zheng"), ('\u{4e66}', "shu"),
    ('\u{7f51}', "wang"), ('\u{7edc}', "luo"), ('\u{8fde}', "lian"), ('\u{63a5}', "jie"),
    ('\u{5b89}', "an"), ('\u{5168}', "quan"), ('\u{52a0}', "jia"), ('\u{5bc6}', "mi"),
    ('\u{89e3}', "jie"), ('\u{538b}', "ya"), ('\u{7f29}', "suo"), ('\u{5c55}', "zhan"),
    ('\u{5f00}', "kai"), ('\u{5173}', "guan"), ('\u{95ed}', "bi"), ('\u{542f}', "qi"),
    ('\u{52a8}', "dong"), ('\u{505c}', "ting"), ('\u{6b62}', "zhi"), ('\u{59cb}', "shi"),
    ('\u{7ec8}', "zhong"), ('\u{7ed3}', "jie"), ('\u{675f}', "shu"), ('\u{5b8c}', "wan"),
    ('\u{6210}', "cheng"), ('\u{521d}', "chu"), ('\u{59cb}', "shi"), ('\u{5316}', "hua"),
    ('\u{6e05}', "qing"), ('\u{7406}', "li"), ('\u{9664}', "chu"), ('\u{5220}', "shan"),
    ('\u{51cf}', "jian"), ('\u{589e}', "zeng"), ('\u{6539}', "gai"), ('\u{4fee}', "xiu"),
    ('\u{590d}', "fu"), ('\u{539f}', "yuan"), ('\u{91cd}', "zhong"), ('\u{5efa}', "jian"),
    ('\u{6784}', "gou"), ('\u{9020}', "zao"), ('\u{751f}', "sheng"), ('\u{4ea7}', "chan"),
    ('\u{6d88}', "xiao"), ('\u{8d39}', "fei"), ('\u{6a21}', "mo"), ('\u{5757}', "kuai"),
    ('\u{529f}', "gong"), ('\u{80fd}', "neng"), ('\u{7279}', "te"), ('\u{6027}', "xing"),
    ('\u{53c2}', "can"), ('\u{6570}', "shu"), ('\u{9009}', "xuan"), ('\u{9879}', "xiang"),
    ('\u{6807}', "biao"), ('\u{7b7e}', "qian"), ('\u{5377}', "juan"), ('\u{533a}', "qu"),
    ('\u{5206}', "fen"), ('\u{5272}', "ge"), ('\u{683c}', "ge"), ('\u{5f0f}', "shi"),
    ('\u{7c7b}', "lei"), ('\u{578b}', "xing"), ('\u{5927}', "da"), ('\u{5c0f}', "xiao"),
    ('\u{957f}', "chang"), ('\u{77ed}', "duan"), ('\u{5bbd}', "kuan"), ('\u{9ad8}', "gao"),
    ('\u{6df1}', "shen"), ('\u{6d45}', "qian"), ('\u{524d}', "qian"), ('\u{540e}', "hou"),
    ('\u{5de6}', "zuo"), ('\u{53f3}', "you"), ('\u{4e0a}', "shang"), ('\u{4e0b}', "xia"),
    ('\u{5185}', "nei"), ('\u{5916}', "wai"), ('\u{4e2d}', "zhong"), ('\u{95f4}', "jian"),
    ('\u{5934}', "tou"), ('\u{5c3e}', "wei"), ('\u{9996}', "shou"), ('\u{672b}', "mo"),
    ('\u{65b0}', "xin"), ('\u{65e7}', "jiu"), ('\u{597d}', "hao"), ('\u{574f}', "huai"),
    ('\u{5bf9}', "dui"), ('\u{9519}', "cuo"), ('\u{771f}', "zhen"), ('\u{5047}', "jia"),
    ('\u{7a7a}', "kong"), ('\u{6ee1}', "man"), ('\u{591a}', "duo"), ('\u{5c11}', "shao"),
    ('\u{5168}', "quan"), ('\u{90e8}', "bu"), ('\u{5206}', "fen"), ('\u{4e2a}', "ge"),
    ('\u{6bcf}', "mei"), ('\u{5404}', "ge"), ('\u{6b64}', "ci"), ('\u{5f7c}', "bi"),
    ('\u{8be5}', "gai"), ('\u{5176}', "qi"), ('\u{53e6}', "ling"), ('\u{5916}', "wai"),
    ('\u{4ed6}', "ta"), ('\u{5979}', "ta"), ('\u{5b83}', "ta"), ('\u{6211}', "wo"),
    ('\u{4f60}', "ni"), ('\u{4eec}', "men"), ('\u{8c01}', "shui"), ('\u{4ec0}', "shen"),
    ('\u{4e48}', "me"), ('\u{54ea}', "na"), ('\u{90a3}', "na"), ('\u{8fd9}', "zhe"),
    ('\u{4e9b}', "xie"), ('\u{6837}', "yang"), ('\u{79cd}', "zhong"), ('\u{7c7b}', "lei"),
    ('\u{884c}', "hang"), ('\u{5217}', "lie"), ('\u{6392}', "pai"), ('\u{5e8f}', "xu"),
    ('\u{7ec4}', "zu"), ('\u{5408}', "he"), ('\u{5206}', "fen"), ('\u{62c6}', "chai"),
    ('\u{4ece}', "cong"), ('\u{5230}', "dao"), ('\u{5411}', "xiang"), ('\u{5f80}', "wang"),
    ('\u{5728}', "zai"), ('\u{4e8e}', "yu"), ('\u{88ab}', "bei"), ('\u{628a}', "ba"),
    ('\u{8ba9}', "rang"), ('\u{4f7f}', "shi"), ('\u{4ee4}', "ling"), ('\u{53eb}', "jiao"),
    ('\u{53ef}', "ke"), ('\u{4ee5}', "yi"), ('\u{80fd}', "neng"), ('\u{4f1a}', "hui"),
    ('\u{987b}', "xu"), ('\u{8981}', "yao"), ('\u{9700}', "xu"), ('\u{60f3}', "xiang"),
    ('\u{89c9}', "jue"), ('\u{5f97}', "de"), ('\u{77e5}', "zhi"), ('\u{9053}', "dao"),
    ('\u{770b}', "kan"), ('\u{89c1}', "jian"), ('\u{542c}', "ting"), ('\u{8bf4}', "shuo"),
    ('\u{8bfb}', "du"), ('\u{5199}', "xie"), ('\u{7b97}', "suan"), ('\u{6570}', "shu"),
    ('\u{627e}', "zhao"), ('\u{5230}', "dao"), ('\u{5bfb}', "xun"), ('\u{641c}', "sou"),
    ('\u{7d22}', "suo"), ('\u{68c0}', "jian"), ('\u{67e5}', "cha"), ('\u{6d4b}', "ce"),
    ('\u{8bd5}', "shi"), ('\u{9a8c}', "yan"), ('\u{8bc1}', "zheng"), ('\u{6821}', "jiao"),
    ('\u{9a8c}', "yan"), ('\u{6bd4}', "bi"), ('\u{8f83}', "jiao"), ('\u{5224}', "pan"),
    ('\u{65ad}', "duan"), ('\u{51b3}', "jue"), ('\u{5b9a}', "ding"), ('\u{62e9}', "ze"),
    ('\u{9009}', "xuan"), ('\u{6311}', "tiao"), ('\u{62e3}', "jian"), ('\u{5f03}', "qi"),
    ('\u{7559}', "liu"), ('\u{5b58}', "cun"), ('\u{53d6}', "qu"), ('\u{6536}', "shou"),
    ('\u{53d1}', "fa"), ('\u{9001}', "song"), ('\u{4f20}', "chuan"), ('\u{9012}', "di"),
    ('\u{4ea4}', "jiao"), ('\u{6362}', "huan"), ('\u{8f6c}', "zhuan"), ('\u{53d8}', "bian"),
    ('\u{66f4}', "geng"), ('\u{65b0}', "xin"), ('\u{66ff}', "ti"), ('\u{4ee3}', "dai"),
    ('\u{5f53}', "dang"), ('\u{524d}', "qian"), ('\u{6b64}', "ci"), ('\u{523b}', "ke"),
    ('\u{73b0}', "xian"), ('\u{5728}', "zai"), ('\u{8fc7}', "guo"), ('\u{53bb}', "qu"),
    ('\u{672a}', "wei"), ('\u{6765}', "lai"), ('\u{6c38}', "yong"), ('\u{8fdc}', "yuan"),
    ('\u{65e9}', "zao"), ('\u{665a}', "wan"), ('\u{5148}', "xian"), ('\u{540e}', "hou"),
    ('\u{5feb}', "kuai"), ('\u{6162}', "man"), ('\u{901f}', "su"), ('\u{8fdf}', "chi"),
    ('\u{540c}', "tong"), ('\u{5f02}', "yi"), ('\u{4f3c}', "si"), ('\u{50cf}', "xiang"),
    ('\u{5982}', "ru"), ('\u{82e5}', "ruo"), ('\u{6216}', "huo"), ('\u{4e14}', "qie"),
    ('\u{5e76}', "bing"), ('\u{4e0e}', "yu"), ('\u{975e}', "fei"), ('\u{5426}', "fou"),
    ('\u{662f}', "shi"), ('\u{5426}', "fou"), ('\u{6709}', "you"), ('\u{65e0}', "wu"),
    ('\u{5b58}', "cun"), ('\u{5728}', "zai"), ('\u{7f3a}', "que"), ('\u{4e4f}', "fa"),
    ('\u{8db3}', "zu"), ('\u{591f}', "gou"), ('\u{591a}', "duo"), ('\u{4f59}', "yu"),
    ('\u{5269}', "sheng"), ('\u{8282}', "jie"), ('\u{70b9}', "dian"), ('\u{5757}', "kuai"),
    ('\u{6bb5}', "duan"), ('\u{533a}', "qu"), ('\u{57df}', "yu"), ('\u{8303}', "fan"),
    ('\u{56f4}', "wei"), ('\u{754c}', "jie"), ('\u{9650}', "xian"), ('\u{8fb9}', "bian"),
    ('\u{754c}', "jie"), ('\u{7aef}', "duan"), ('\u{53e3}', "kou"), ('\u{5f84}', "jing"),
    ('\u{8def}', "lu"), ('\u{5f84}', "jing"), ('\u{9053}', "dao"), ('\u{8f68}', "gui"),
    ('\u{8ff9}', "ji"), ('\u{7ebf}', "xian"), ('\u{9762}', "mian"), ('\u{5c42}', "ceng"),
    ('\u{7ea7}', "ji"), ('\u{9636}', "jie"), ('\u{6b65}', "bu"), ('\u{6bb5}', "duan"),
    ('\u{6b21}', "ci"), ('\u{56de}', "hui"), ('\u{904d}', "bian"), ('\u{5386}', "li"),
    ('\u{7ecf}', "jing"), ('\u{8fc7}', "guo"), ('\u{9014}', "tu"), ('\u{5f84}', "jing"),
    ('\u{65b9}', "fang"), ('\u{6cd5}', "fa"), ('\u{5f0f}', "shi"), ('\u{6837}', "yang"),
    ('\u{5f62}', "xing"), ('\u{72b6}', "zhuang"), ('\u{6001}', "tai"), ('\u{76f8}', "xiang"),
    ('\u{72b6}', "zhuang"), ('\u{6001}', "tai"), ('\u{666f}', "jing"), ('\u{60c5}', "qing"),
    ('\u{51b5}', "kuang"), ('\u{6761}', "tiao"), ('\u{4ef6}', "jian"), ('\u{89c4}', "gui"),
    ('\u{5219}', "ze"), ('\u{5f8b}', "lv"), ('\u{6cd5}', "fa"), ('\u{5f8b}', "lv"),
    ('\u{6807}', "biao"), ('\u{51c6}', "zhun"), ('\u{89c4}', "gui"), ('\u{8303}', "fan"),
    ('\u{7ea6}', "yue"), ('\u{5b9a}', "ding"), ('\u{9ed8}', "mo"), ('\u{8ba4}', "ren"),
    ('\u{7f3a}', "que"), ('\u{7701}', "sheng"), ('\u{56de}', "hui"), ('\u{9000}', "tui"),
    ('\u{964d}', "jiang"), ('\u{7ea7}', "ji"), ('\u{63d0}', "ti"), ('\u{5347}', "sheng"),
    ('\u{589e}', "zeng"), ('\u{52a0}', "jia"), ('\u{51cf}', "jian"), ('\u{5c11}', "shao"),
    ('\u{5220}', "shan"), ('\u{9664}', "chu"), ('\u{79fb}', "yi"), ('\u{52a8}', "dong"),
    ('\u{62d6}', "tuo"), ('\u{62c9}', "la"), ('\u{63a8}', "tui"), ('\u{9001}', "song"),
    ('\u{6309}', "an"), ('\u{538b}', "ya"), ('\u{5f39}', "tan"), ('\u{653e}', "fang"),
    ('\u{677e}', "song"), ('\u{7d27}', "jin"), ('\u{56fa}', "gu"), ('\u{5b9a}', "ding"),
    ('\u{9501}', "suo"), ('\u{89e3}', "jie"), ('\u{5f00}', "kai"), ('\u{5c01}', "feng"),
    ('\u{62c6}', "chai"), ('\u{88c5}', "zhuang"), ('\u{5378}', "xie"), ('\u{8f7d}', "zai"),
    ('\u{6302}', "gua"), ('\u{8f7d}', "zai"), ('\u{5378}', "xie"), ('\u{8f7d}', "zai"),
    ('\u{6620}', "ying"), ('\u{5c04}', "she"), ('\u{5173}', "guan"), ('\u{8054}', "lian"),
    ('\u{94fe}', "lian"), ('\u{63a5}', "jie"), ('\u{65ad}', "duan"), ('\u{5f00}', "kai"),
    ('\u{5408}', "he"), ('\u{5e76}', "bing"), ('\u{5206}', "fen"), ('\u{88c2}', "lie"),
    ('\u{62c6}', "chai"), ('\u{5206}', "fen"), ('\u{7ec4}', "zu"), ('\u{88c5}', "zhuang"),
    // 常见文件名用字
    ('\u{8d44}', "zi"), ('\u{6e90}', "yuan"), ('\u{8d44}', "zi"), ('\u{4ea7}', "chan"),
    ('\u{8d44}', "zi"), ('\u{6599}', "liao"), ('\u{4fe1}', "xin"), ('\u{606f}', "xi"),
    ('\u{5e2e}', "bang"), ('\u{52a9}', "zhu"), ('\u{6307}', "zhi"), ('\u{5357}', "nan"),
    ('\u{624b}', "shou"), ('\u{518c}', "ce"), ('\u{8bf4}', "shuo"), ('\u{660e}', "ming"),
    ('\u{63cf}', "miao"), ('\u{8ff0}', "shu"), ('\u{6ce8}', "zhu"), ('\u{91ca}', "shi"),
    ('\u{5907}', "bei"), ('\u{6ce8}', "zhu"), ('\u{6807}', "biao"), ('\u{8bb0}', "ji"),
    ('\u{7b26}', "fu"), ('\u{53f7}', "hao"), ('\u{7f16}', "bian"), ('\u{53f7}', "hao"),
    ('\u{5e8f}', "xu"), ('\u{5217}', "lie"), ('\u{961f}', "dui"), ('\u{5217}', "lie"),
    ('\u{6392}', "pai"), ('\u{884c}', "hang"), ('\u{77e9}', "ju"), ('\u{9635}', "zhen"),
    ('\u{8868}', "biao"), ('\u{683c}', "ge"), ('\u{56fe}', "tu"), ('\u{8868}', "biao"),
    ('\u{56fe}', "tu"), ('\u{50cf}', "xiang"), ('\u{89c6}', "shi"), ('\u{56fe}', "tu"),
    ('\u{8349}', "cao"), ('\u{7a3f}', "gao"), ('\u{539f}', "yuan"), ('\u{7a3f}', "gao"),
    ('\u{6b63}', "zheng"), ('\u{7a3f}', "gao"), ('\u{526f}', "fu"), ('\u{7a3f}', "gao"),
    ('\u{5907}', "bei"), ('\u{4efd}', "fen"), ('\u{5f52}', "gui"), ('\u{6863}', "dang"),
    ('\u{5b58}', "cun"), ('\u{6863}', "dang"), ('\u{5efa}', "jian"), ('\u{6863}', "dang"),
    ('\u{6587}', "wen"), ('\u{6863}', "dang"), ('\u{6848}', "an"), ('\u{5377}', "juan"),
    ('\u{5b97}', "zong"), ('\u{4ef6}', "jian"), ('\u{6848}', "an"), ('\u{5377}', "juan"),
    // 常见系统分区用字
    ('\u{4ea7}', "chan"), ('\u{54c1}', "pin"), ('\u{5382}', "chang"), ('\u{5546}', "shang"),
    ('\u{9a71}', "qu"), ('\u{52a8}', "dong"), ('\u{56fa}', "gu"), ('\u{4ef6}', "jian"),
    ('\u{786c}', "ying"), ('\u{76d8}', "pan"), ('\u{5185}', "nei"), ('\u{5b58}', "cun"),
    ('\u{95ea}', "shan"), ('\u{5b58}', "cun"), ('\u{5b58}', "cun"), ('\u{50a8}', "chu"),
    ('\u{5361}', "ka"), ('\u{76d8}', "pan"), ('\u{5377}', "juan"), ('\u{533a}', "qu"),
    ('\u{5f15}', "yin"), ('\u{5bfc}', "dao"), ('\u{542f}', "qi"), ('\u{52a8}', "dong"),
    ('\u{6062}', "hui"), ('\u{590d}', "fu"), ('\u{5237}', "shua"), ('\u{673a}', "ji"),
    ('\u{57fa}', "ji"), ('\u{5e26}', "dai"), ('\u{8c03}', "diao"), ('\u{5236}', "zhi"),
    ('\u{89e3}', "jie"), ('\u{8c03}', "diao"), ('\u{5668}', "qi"), ('\u{82af}', "xin"),
    ('\u{7247}', "pian"), ('\u{6a21}', "mo"), ('\u{7ec4}', "zu"), ('\u{5957}', "tao"),
    ('\u{7fa4}', "qun"), ('\u{96c6}', "ji"), ('\u{7c07}', "cu"), ('\u{5757}', "kuai")
];