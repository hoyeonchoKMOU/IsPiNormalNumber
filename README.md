# Is Pi a Normal Number?

Pi의 소수점 자릿수를 실시간으로 계산하며, 각 숫자(0~9)의 출현 빈도를 시각화하여 Pi가 **Normal Number**인지 실험적으로 검증하는 터미널 프로그램입니다.

> **Normal Number**: 모든 자릿수 조합이 균등한 확률로 출현하는 수. Pi가 Normal Number인지는 수학적으로 아직 미해결 문제(open problem)이지만, 실험적 증거는 이를 강하게 지지합니다.

![Screenshot](Screenshot.png)

## Features

- **Chudnovsky Binary Splitting Algorithm** - Pi 세계 기록 수립에 사용되는 동일 알고리즘
- **실시간 막대 그래프** - 0~9 각 숫자의 출현 횟수, 비율(%), 10%로부터의 편차를 색상별로 시각화
- **통계적 검정**
  - **Chi-squared (χ²)** - 균일분포 적합도 검정 (df=9, α=0.05)
  - **Shannon Entropy** - 이론적 최대값 log₂(10) ≈ 3.3219 bits 대비 수렴도
  - **Max |deviation|** - 10%로부터의 최대 편차
- **수렴 스파크라인** - 자릿수가 증가함에 따라 각 지표가 균일분포에 수렴하는 과정을 시각화
- **깜빡임 없는 렌더링** - 커서 이동 + 줄 끝 지우기 방식으로 화면 전체를 지우지 않음

## Quick Start

```bash
# Build
cargo build --release

# Run
cargo run --release
```

`Ctrl+C` 또는 `ESC`로 종료합니다.

## How It Works

### 알고리즘

[Chudnovsky formula](https://en.wikipedia.org/wiki/Chudnovsky_algorithm)를 Binary Splitting 기법으로 구현합니다:

$$\frac{1}{\pi} = 12 \sum_{k=0}^{\infty} \frac{(-1)^k (6k)! (13591409 + 545140134k)}{(3k)!(k!)^3 \, 640320^{3k+3/2}}$$

- 1회 계산으로 약 14.18자리의 정밀도 확보
- 배치 크기를 1,000 → 2,000 → 4,000 → ... → 2,000,000으로 점진적으로 증가
- 백그라운드 스레드에서 계산, 메인 스레드에서 시각화

### 통계 검정

| 지표 | 기준 | 의미 |
|---|---|---|
| χ² < 16.919 | df=9, p=0.05 | 균일분포와 통계적으로 구분 불가 → **UNIFORM** |
| Entropy → 3.3219 | log₂(10) bits | 각 숫자의 출현 확률이 균등 |
| \|dev\|max → 0 | 10%로부터의 편차 | 모든 숫자가 정확히 10%에 수렴 |

## Results (1,024,000 digits)

```
χ² = 6.574    UNIFORM
Entropy: 3.3219/3.3219 bits (100.00%)
|dev|max: 0.047%
```

100만 자리 수준에서 0~9 각 숫자가 거의 정확히 10%씩 출현하며, 모든 통계 검정을 통과합니다.

## Dependencies

- [`num-bigint`](https://crates.io/crates/num-bigint) - 임의 정밀도 정수 연산
- [`num-traits`](https://crates.io/crates/num-traits) - 수치 트레이트
- [`crossterm`](https://crates.io/crates/crossterm) - 크로스 플랫폼 터미널 조작

## License

MIT
