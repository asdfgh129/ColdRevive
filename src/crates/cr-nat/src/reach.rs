//! 可达性类别声明（`ReachClass`）—— 借 eMule 的 HighID / LowID 做法。
//!
//! # 借的是什么
//!
//! eMule 里每个客户端**自己知道**自己是 HighID 还是 LowID，并在与服务器建立会话时
//! **声明**它；别的节点看到「这个源是 LowID」，就知道「直接连它不可能」，不会先去
//! 试一遍再超时。失败是**预先知道**的，不是**试出来**的。
//!
//! 本模块把这件事搬过来：一个节点的可达性类别由它自己的 NAT 实测结果得出，
//! 带**时间戳与有效期**，随注册声明给中继，中继可在实例间同步。
//!
//! # 没借的是什么（这条比上面更重要）
//!
//! eMule 的 LowID 是**终点**：连不上就是连不上，只能换一个源。我们没有「换一个源」这条路——
//! 要叫醒的就是那一台机器，没有替补。所以本模块的 `RelayOnly` 不是「放弃」，
//! 而是「**别再浪费一次注定超时的直连尝试，从中继走**」。中继是本产品的第二条路，
//! 不是失败兜底。
//!
//! 还有一处必须说清的语义差别：
//!
//! - eMule 的 HighID 含义是「**任何人**现在就能主动连进来」（需要端口映射/公网可达）；
//! - 本模块的 `Direct` 含义是「**在双方配合打洞的前提下**能建立直连」。
//!
//! 端口受限锥型 NAT 就属于后者：它对陌生来源不放行，但只要我方先向对端发出一个包，
//! 对端的回包就能进来（W3.6 实验室 30/30 实测）。它**不是** HighID 那种「谁都能连」，
//! 因此这个名字不能误读成「这机器暴露在公网上」。
//!
//! # 为什么类别必须带有效期
//!
//! 类别是**光猫的当前配置行为**，不是机器的固有属性：换光猫、改桥接、运营商下发新配置、
//! 甚至重启后 CPE 换了 NAT 模式，都会让结论失效。声明里没有时间戳，就等于把一次探测
//! 永久当成事实——这正是本项目反复在打的那类缺陷（拿旧观测冒充当前状态）。
//! 过期后一律降级为 `Unknown`，**不做任何推断**。

use crate::{NatType, TraversalReport};

/// 默认有效期：1 小时。
///
/// 取值理由：太短会让每个进程都反复探测（STUN 往返 + 过滤行为测试，秒级开销），
/// 太长则跨过一次「光猫重启/换配置」还不自知。1 小时对家用环境是够用的折中；
/// 这是工程取舍，不是测量结论，因此写成常量而不是散落在各处的字面量。
pub const DEFAULT_TTL_MS: u64 = 60 * 60 * 1000;

/// 线缆编码：`Unknown`。
pub const WIRE_UNKNOWN: u8 = 0;
/// 线缆编码：`Direct`。
pub const WIRE_DIRECT: u8 = 1;
/// 线缆编码：`RelayOnly`。
pub const WIRE_RELAY_ONLY: u8 = 2;

/// 当前时间（毫秒，Unix epoch）。取不到时钟时返回 0 —— 0 会让声明立刻过期，
/// 这是**故意选的方向**：时钟不可信时宁可不信任声明，也不要信任一个假的「刚刚测过」。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 一个节点的可达性类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachClass {
    /// 双方配合打洞后可以直连（锥形 NAT）。**不等于**「谁都能主动连进来」。
    Direct,
    /// 打洞在原理上无法成功（对称型：映射随目的变，对端无从得知该打哪个端口），只能走中继。
    RelayOnly,
    /// 没测过 / 测了但不足以判定 / 声明已过期。**不推断**，调用方按「两条路都试」处理。
    Unknown,
}

impl ReachClass {
    pub fn label(&self) -> &'static str {
        match self {
            ReachClass::Direct => "direct",
            ReachClass::RelayOnly => "relay-only",
            ReachClass::Unknown => "unknown",
        }
    }

    pub fn wire(&self) -> u8 {
        match self {
            ReachClass::Direct => WIRE_DIRECT,
            ReachClass::RelayOnly => WIRE_RELAY_ONLY,
            ReachClass::Unknown => WIRE_UNKNOWN,
        }
    }

    /// 未知编码一律当 `Unknown`，**不猜**。协议扩展时新编码在老客户端上会走这里。
    pub fn from_wire(b: u8) -> ReachClass {
        match b {
            WIRE_DIRECT => ReachClass::Direct,
            WIRE_RELAY_ONLY => ReachClass::RelayOnly,
            _ => ReachClass::Unknown,
        }
    }

    /// 这条声明是否值得据此**跳过**一次直连尝试。
    ///
    /// 只有 `RelayOnly` 才算「确定不该直连」。`Unknown` 绝不能当 `RelayOnly` 用——
    /// 那会把「没测过」变成「连不上」，是本项目明令禁止的那类替换。
    pub fn allows_skipping_direct(&self) -> bool {
        matches!(self, ReachClass::RelayOnly)
    }
}

/// 一次可达性声明：类别 + 依据 + 何时测的 + 多久算过期。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachDeclaration {
    pub class: ReachClass,
    /// 得出该类别时的 NAT 类型（`Unknown` 表示当时也没测出来）。
    pub nat_class: NatType,
    /// 声明产生的时刻（ms, Unix epoch）。
    pub declared_at_ms: u64,
    /// 有效期（ms）。
    pub ttl_ms: u64,
}

impl ReachDeclaration {
    /// 「没测过」的声明。**这是默认值**：守护进程刚起、从没跑过穿透探测时就是它。
    pub fn unknown(now: u64) -> Self {
        ReachDeclaration {
            class: ReachClass::Unknown,
            nat_class: NatType::Unknown,
            declared_at_ms: now,
            ttl_ms: DEFAULT_TTL_MS,
        }
    }

    /// 由一次真实的穿透探测结果得出声明。
    ///
    /// 映射关系（每条都能说出依据）：
    ///
    /// | NAT 类型 | 类别 | 依据 |
    /// | :--- | :--- | :--- |
    /// | `symmetric` | `RelayOnly` | 映射随目的地址变化，对端无从得知该打哪个端口；W3.7 抓包已确证 |
    /// | `full-cone` / `restricted-cone` / `port-restricted-cone` | `Direct` | 映射与目的无关，配合打洞可建立直连；W3.6 逐类实测 |
    /// | `cgnat` | **`Unknown`** | CGNAT 的实际行为取决于运营商侧那一层，**不能按名字推断** |
    /// | `unknown` | `Unknown` | 样本不足，说不出来就不说 |
    ///
    /// `cgnat` 这一行是刻意保守的：实验室里合成的双层 CGNAT 直连 30/30，
    /// 真实运营商 CGNAT 常常是对称型——两边都真实存在，所以「cgnat」这个名字本身
    /// 不携带可达性信息。要判定只能实测。
    pub fn from_report(report: &TraversalReport, now: u64) -> Self {
        let (class, nat_class) = if !report.public_udp_egress_ok {
            // 连映射都没拿到：既没证据说能直连，也没证据说不能，只能 Unknown。
            (ReachClass::Unknown, report.nat_type)
        } else {
            match report.nat_type {
                NatType::Symmetric => (ReachClass::RelayOnly, NatType::Symmetric),
                NatType::FullCone => (ReachClass::Direct, NatType::FullCone),
                NatType::RestrictedCone => (ReachClass::Direct, NatType::RestrictedCone),
                NatType::PortRestrictedCone => {
                    (ReachClass::Direct, NatType::PortRestrictedCone)
                }
                // 端口过滤已实测不存在、地址轴未测：打洞只依赖「映射可预测 + 不禁异端口」，
                // 地址是否受限不改变「可预测直连」这个决策，故与 cone 同判 Direct。
                // 类型名仍保留问号（ConeUndetermined），不升级成 full-cone。
                NatType::ConeUndetermined => (ReachClass::Direct, NatType::ConeUndetermined),
                NatType::Cgnat => (ReachClass::Unknown, NatType::Cgnat),
                NatType::Unknown => (ReachClass::Unknown, NatType::Unknown),
            }
        };
        ReachDeclaration {
            class,
            nat_class,
            declared_at_ms: now,
            ttl_ms: DEFAULT_TTL_MS,
        }
    }

    pub fn with_ttl(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = ttl_ms;
        self
    }

    /// 过期判定。时钟回拨（`now < declared_at`）也算过期——不能因为系统时间倒退
    /// 就让一份旧声明永远有效。
    pub fn is_expired(&self, now: u64) -> bool {
        if now < self.declared_at_ms {
            return true;
        }
        let age = now - self.declared_at_ms;
        age > self.ttl_ms
    }

    /// 调用方真正该用的类别：过期即 `Unknown`，**不返回过期值**。
    pub fn effective_at(&self, now: u64) -> ReachClass {
        if self.is_expired(now) {
            ReachClass::Unknown
        } else {
            self.class
        }
    }

    /// 声明是否满足「据此跳过直连」的条件（未过期 + 类别为 `RelayOnly`）。
    pub fn may_skip_direct(&self, now: u64) -> bool {
        self.effective_at(now).allows_skipping_direct()
    }

    /// 剩余有效毫秒数（已过期返回 0）。
    pub fn remaining_ms(&self, now: u64) -> u64 {
        if self.is_expired(now) {
            0
        } else {
            self.declared_at_ms + self.ttl_ms - now
        }
    }

    pub fn to_json(&self) -> cr_core::Json {
        use cr_core::Json;
        Json::obj(vec![
            ("class", Json::s(self.class.label())),
            ("nat_class", Json::s(self.nat_class.label())),
            ("declared_at_ms", Json::n(self.declared_at_ms as f64)),
            ("ttl_ms", Json::n(self.ttl_ms as f64)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilterBehavior, Strategy};

    fn report(nat: NatType, egress_ok: bool) -> TraversalReport {
        TraversalReport {
            nat_type: nat,
            public_udp_egress_ok: egress_ok,
            upnp_available: false,
            upnp_external_ip: None,
            natpmp_available: false,
            natpmp_external_ip: None,
            pcp_available: false,
            cgnat_suspected: false,
            mapped_address: None,
            recommended_strategy: Strategy::Relay,
            filter_behavior: FilterBehavior::Unknown,
            observations: Vec::new(),
        }
    }

    #[test]
    fn symmetric_is_relay_only() {
        let d = ReachDeclaration::from_report(&report(NatType::Symmetric, true), 1_000);
        assert_eq!(d.class, ReachClass::RelayOnly);
        assert!(d.may_skip_direct(1_000));
    }

    #[test]
    fn all_three_cone_types_are_direct() {
        for nat in [
            NatType::FullCone,
            NatType::RestrictedCone,
            NatType::PortRestrictedCone,
        ] {
            let d = ReachDeclaration::from_report(&report(nat.clone(), true), 1_000);
            assert_eq!(d.class, ReachClass::Direct, "{}", nat.label());
            assert!(!d.may_skip_direct(1_000));
        }
    }

    /// CGNAT 不能按名字推断可达性：实验室合成的双层 CGNAT 直连 30/30，
    /// 真实运营商 CGNAT 常常是对称型。所以这个名字只配拿到 Unknown。
    #[test]
    fn cgnat_is_not_auto_classified() {
        let d = ReachDeclaration::from_report(&report(NatType::Cgnat, true), 1_000);
        assert_eq!(d.class, ReachClass::Unknown);
        assert!(!d.may_skip_direct(1_000));
    }

    #[test]
    fn no_udp_egress_means_unknown_even_if_type_says_cone() {
        // 理论上的矛盾输入：类型说锥形，但连映射都没拿到。
        // 结论必须服从「有没有证据」，而不是服从更乐观的那个字段。
        let d = ReachDeclaration::from_report(&report(NatType::FullCone, false), 1_000);
        assert_eq!(d.class, ReachClass::Unknown);
    }

    #[test]
    fn expired_declaration_degrades_to_unknown() {
        let d = ReachDeclaration::from_report(&report(NatType::Symmetric, true), 1_000)
            .with_ttl(500);
        assert_eq!(d.effective_at(1_400), ReachClass::RelayOnly);
        assert_eq!(d.effective_at(1_600), ReachClass::Unknown);
        assert!(!d.may_skip_direct(1_600));
        assert_eq!(d.remaining_ms(1_600), 0);
    }

    /// 系统时间倒退不能让旧声明永远有效 —— 否则一份过期声明会永久生效。
    #[test]
    fn clock_going_backwards_counts_as_expired() {
        let d = ReachDeclaration::from_report(&report(NatType::Symmetric, true), 10_000);
        assert!(d.is_expired(9_000));
        assert_eq!(d.effective_at(9_000), ReachClass::Unknown);
    }

    #[test]
    fn unknown_never_authorises_skipping_direct() {
        let d = ReachDeclaration::unknown(1_000);
        assert!(!d.may_skip_direct(1_000));
        assert!(!ReachClass::Unknown.allows_skipping_direct());
    }

    #[test]
    fn wire_codes_roundtrip_and_unknown_codes_degrade() {
        for c in [
            ReachClass::Direct,
            ReachClass::RelayOnly,
            ReachClass::Unknown,
        ] {
            assert_eq!(ReachClass::from_wire(c.wire()), c);
        }
        // 老版本读到新编码：不猜，一律 Unknown。
        assert_eq!(ReachClass::from_wire(0x7f), ReachClass::Unknown);
    }

    #[test]
    fn json_carries_class_and_basis() {
        let d = ReachDeclaration::from_report(&report(NatType::PortRestrictedCone, true), 1_700_000_000_000);
        let j = d.to_json().dump();
        assert!(j.contains("\"class\":\"direct\""), "{}", j);
        assert!(j.contains("\"nat_class\":\"port-restricted-cone\""), "{}", j);
        assert!(j.contains("\"ttl_ms\":3600000"), "{}", j);
    }

    /// 默认 TTL 必须大于 0：TTL=0 会让每条声明在产生的同一毫秒就过期，
    /// 表现为「功能永远不生效」，而且从日志上看不出任何错误。
    #[test]
    fn default_ttl_is_usable() {
        assert!(DEFAULT_TTL_MS >= 60_000);
        let d = ReachDeclaration::unknown(0);
        assert!(!d.is_expired(1));
    }
}
