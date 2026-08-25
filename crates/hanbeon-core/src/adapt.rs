//! 상황적응 주사 간격.
//!
//! 같은 사용자도 오전과 오후의 반응속도가 다르고, 근긴장·통증·피로에 따라
//! 입력 시간이 달라진다. 최근 기록을 보고 간격을 조금씩 옮기되, 아래 세 가지를
//! 지킨다(PRD F5, 원칙 1·2·6).
//!
//! 1. 사용자가 정한 최소·최대를 절대 벗어나지 않는다.
//! 2. 한 번에 ±10% 이상 바꾸지 않는다. 급변은 익힌 타이밍을 무너뜨린다.
//! 3. 되돌리기나 놓침이 나오면 즉시 느슨하게 하고, 한동안 조이지 않는다.

use std::collections::VecDeque;

/// 반응 기록을 몇 개까지 보고 판단할지.
const WINDOW: usize = 10;

/// 이만큼은 모여야 조정을 시작한다. 한두 번의 우연으로 간격이 흔들리면 안 된다.
const MIN_SAMPLES: usize = 3;

/// 반응시간에 얹는 여유. 딱 맞추면 매번 아슬아슬해진다.
const MARGIN_MS: u64 = 300;

/// 한 번에 움직일 수 있는 최대 비율(퍼센트).
const MAX_STEP_PERCENT: u64 = 10;

/// 되돌리기·놓침이 나왔을 때 늘리는 비율(퍼센트).
const SAFETY_PERCENT: u64 = 125;

/// 안전값을 준 뒤 이 횟수만큼은 줄이는 방향으로 가지 않는다.
const COOLDOWN: u32 = 3;

/// 한 번의 선택에서 관측한 것.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 커서가 칸에 들어온 뒤 스위치를 누르기까지 걸린 시간.
    pub reaction_ms: u64,
    /// 이 선택이 되돌리기였다.
    pub undo: bool,
    /// 한 바퀴 이상 돌고 나서야 선택했다.
    pub missed: bool,
}

/// 간격이 왜 바뀌었는지. 사용자에게 그대로 보여준다(PRD 원칙 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// 반응이 빨라져 간격을 줄였다.
    Faster,
    /// 반응이 느려져 간격을 늘렸다.
    Slower,
    /// 실수가 감지되어 안전하게 늘렸다.
    Safety,
}

impl Reason {
    pub fn describe(self, from_ms: u64, to_ms: u64) -> String {
        let from = from_ms as f64 / 1000.0;
        let to = to_ms as f64 / 1000.0;
        match self {
            Reason::Faster => format!("최근 반응이 빨라져 {from:.1}초 → {to:.1}초"),
            Reason::Slower => format!("최근 반응이 느려져 {from:.1}초 → {to:.1}초"),
            Reason::Safety => format!("실수가 감지되어 {from:.1}초 → {to:.1}초"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Adjustment {
    pub from_ms: u64,
    pub to_ms: u64,
    pub reason: Reason,
}

/// 사용자가 정한 경계. 적응 로직은 이 밖으로 나갈 수 없다.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub min_ms: u64,
    pub max_ms: u64,
}

#[derive(Debug, Default)]
pub struct Adapter {
    reactions: VecDeque<u64>,
    cooldown: u32,
}

impl Adapter {
    /// 한 번의 선택을 반영하고, 간격을 바꿔야 하면 그 내용을 돌려준다.
    ///
    /// 호출하는 쪽은 수동 고정·적응 꺼짐 여부를 먼저 확인한다. 여기서는
    /// '조정해도 되는 상황'이라고 전제한다.
    pub fn observe(
        &mut self,
        sample: Sample,
        current_ms: u64,
        limits: &Limits,
    ) -> Option<Adjustment> {
        // 되돌리기까지 걸린 시간이나 한 바퀴 놓친 시간은 '반응속도'가 아니다.
        // 평균에 섞으면 실제보다 느린 사용자로 오해하게 된다.
        if sample.undo || sample.missed {
            return self.relax(current_ms, limits);
        }

        self.reactions.push_back(sample.reaction_ms);
        if self.reactions.len() > WINDOW {
            self.reactions.pop_front();
        }

        self.converge(current_ms, limits)
    }

    /// 실수가 보이면 먼저 느슨하게 한다.
    /// 한 번의 실수가 과업 포기로 이어지지 않아야 한다(PRD 원칙 5).
    fn relax(&mut self, current_ms: u64, limits: &Limits) -> Option<Adjustment> {
        self.cooldown = COOLDOWN;

        let target = (current_ms * SAFETY_PERCENT / 100).min(limits.max_ms);
        if target <= current_ms {
            return None;
        }

        Some(Adjustment {
            from_ms: current_ms,
            to_ms: target,
            reason: Reason::Safety,
        })
    }

    /// 최근 반응시간 쪽으로 조금씩 옮긴다.
    fn converge(&mut self, current_ms: u64, limits: &Limits) -> Option<Adjustment> {
        if self.reactions.len() < MIN_SAMPLES {
            return None;
        }

        let average = self.reactions.iter().sum::<u64>() / self.reactions.len() as u64;
        let target = (average + MARGIN_MS).clamp(limits.min_ms, limits.max_ms);

        let step = (current_ms * MAX_STEP_PERCENT / 100).max(1);
        let next = if target > current_ms {
            (current_ms + step).min(target)
        } else {
            // 안전값을 준 직후에는 조이지 않는다. 다시 실수할 여유를 준다.
            if self.cooldown > 0 {
                self.cooldown -= 1;
                return None;
            }
            current_ms.saturating_sub(step).max(target)
        }
        .clamp(limits.min_ms, limits.max_ms);

        if next == current_ms {
            return None;
        }

        Some(Adjustment {
            from_ms: current_ms,
            to_ms: next,
            reason: if next > current_ms {
                Reason::Slower
            } else {
                Reason::Faster
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        min_ms: 800,
        max_ms: 4000,
    };

    fn clean(reaction_ms: u64) -> Sample {
        Sample {
            reaction_ms,
            undo: false,
            missed: false,
        }
    }

    #[test]
    fn 표본이_모자라면_건드리지_않는다() {
        let mut adapter = Adapter::default();
        assert!(adapter.observe(clean(500), 1800, &LIMITS).is_none());
        assert!(adapter.observe(clean(500), 1800, &LIMITS).is_none());
    }

    #[test]
    fn 반응이_빠르면_간격을_줄인다() {
        let mut adapter = Adapter::default();
        adapter.observe(clean(400), 1800, &LIMITS);
        adapter.observe(clean(400), 1800, &LIMITS);
        let adjustment = adapter.observe(clean(400), 1800, &LIMITS).unwrap();

        assert_eq!(adjustment.reason, Reason::Faster);
        assert!(adjustment.to_ms < 1800);
    }

    #[test]
    fn 반응이_느리면_간격을_늘린다() {
        let mut adapter = Adapter::default();
        adapter.observe(clean(3000), 1800, &LIMITS);
        adapter.observe(clean(3000), 1800, &LIMITS);
        let adjustment = adapter.observe(clean(3000), 1800, &LIMITS).unwrap();

        assert_eq!(adjustment.reason, Reason::Slower);
        assert!(adjustment.to_ms > 1800);
    }

    #[test]
    fn 한_번에_십_퍼센트를_넘지_않는다() {
        let mut adapter = Adapter::default();
        for _ in 0..3 {
            adapter.observe(clean(50), 2000, &LIMITS);
        }
        let adjustment = adapter.observe(clean(50), 2000, &LIMITS).unwrap();

        // 목표는 하한(800ms)이지만 한 걸음은 200ms까지다.
        assert_eq!(adjustment.to_ms, 1800);
    }

    #[test]
    fn 사용자가_정한_범위를_벗어나지_않는다() {
        let mut adapter = Adapter::default();
        for _ in 0..5 {
            adapter.observe(clean(50), 900, &LIMITS);
        }
        let adjustment = adapter.observe(clean(50), 900, &LIMITS).unwrap();
        assert!(adjustment.to_ms >= LIMITS.min_ms);

        let mut adapter = Adapter::default();
        for _ in 0..5 {
            adapter.observe(clean(9000), 3900, &LIMITS);
        }
        let adjustment = adapter.observe(clean(9000), 3900, &LIMITS).unwrap();
        assert!(adjustment.to_ms <= LIMITS.max_ms);
    }

    #[test]
    fn 되돌리기가_나오면_즉시_느슨해진다() {
        let mut adapter = Adapter::default();
        let adjustment = adapter
            .observe(
                Sample {
                    reaction_ms: 400,
                    undo: true,
                    missed: false,
                },
                1600,
                &LIMITS,
            )
            .unwrap();

        assert_eq!(adjustment.reason, Reason::Safety);
        assert_eq!(adjustment.to_ms, 2000);
    }

    #[test]
    fn 놓침도_안전값을_부른다() {
        let mut adapter = Adapter::default();
        let adjustment = adapter
            .observe(
                Sample {
                    reaction_ms: 400,
                    undo: false,
                    missed: true,
                },
                1600,
                &LIMITS,
            )
            .unwrap();
        assert_eq!(adjustment.reason, Reason::Safety);
    }

    #[test]
    fn 안전값_직후에는_다시_조이지_않는다() {
        let mut adapter = Adapter::default();
        for _ in 0..3 {
            adapter.observe(clean(300), 2000, &LIMITS);
        }
        adapter.observe(
            Sample {
                reaction_ms: 300,
                undo: true,
                missed: false,
            },
            2000,
            &LIMITS,
        );

        // 반응이 빨라도 세 번은 줄이지 않는다.
        assert!(adapter.observe(clean(300), 2500, &LIMITS).is_none());
        assert!(adapter.observe(clean(300), 2500, &LIMITS).is_none());
        assert!(adapter.observe(clean(300), 2500, &LIMITS).is_none());
        assert!(adapter.observe(clean(300), 2500, &LIMITS).is_some());
    }

    #[test]
    fn 이미_상한이면_안전값으로도_더_늘리지_않는다() {
        let mut adapter = Adapter::default();
        let adjustment = adapter.observe(
            Sample {
                reaction_ms: 400,
                undo: true,
                missed: false,
            },
            LIMITS.max_ms,
            &LIMITS,
        );
        assert!(adjustment.is_none());
    }

    #[test]
    fn 변경_이유는_사람이_읽을_수_있다() {
        assert_eq!(
            Reason::Faster.describe(1800, 1700),
            "최근 반응이 빨라져 1.8초 → 1.7초"
        );
    }
}
