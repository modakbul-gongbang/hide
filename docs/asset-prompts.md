# 펫 캐릭터 에셋 생성 프롬프트

herdr-pet의 상태별 캐릭터 이미지를 생성하기 위한 프롬프트 모음이다.
관련 결정: D-24(생물 펫), D-37(정지 포즈 규격), D-38(상태 8종), D-35(테마 규격).

## 먼저 읽을 것

- **정지 이미지만 만든다.** 애니메이션은 코드가 transform으로 만든다. 스프라이트 시트는 프레임 간 일관성이 무너지므로 쓰지 않는다.
- **1번(idle)을 먼저 확정한다.** 마음에 들 때까지 뽑은 뒤, 그 이미지를 참조 이미지로 첨부해서 나머지 7장을 생성한다.
- **참조 첨부 없이 2번부터 생성하면 매번 다른 캐릭터가 나온다.** 이게 가장 흔한 실패다.
- 모든 프롬프트에 `same scale and margin as the reference`가 들어 있다. 이게 빠지면 상태를 전환할 때 캐릭터 크기가 펄쩍 뛴다.

## 공통 규격

| 항목 | 값 |
|---|---|
| 캔버스 | 1024 x 1024 정사각 |
| 배경 | 투명 (필수) |
| 캐릭터 위치 | 정중앙, 사방 여백 15% |
| 출력 | PNG |
| 스타일 | 8장 전부 동일 |

여백 15%는 에스컬레이션 단계에서 펫이 커지고 흔들릴 때 잘리지 않기 위한 것이다.

## 캐릭터 형상

기본 제안은 **슬라임**이다. 형태가 단순해서 생성 모델이 8장에 걸쳐 일관성을 유지하기 가장 쉽고, 표정과 몸짓 변형도 자유롭다.
강아지나 고양이로 바꾸려면 각 프롬프트의 `round slime creature`를 `puppy` / `cat`으로 바꾸면 된다.
다만 털 있는 동물은 무늬와 품종이 장마다 미묘하게 흔들리므로 참조 이미지 첨부가 더 중요해진다.

---

## 1. idle — 대기도 작업도 없음

가장 먼저, 이것만 반복해서 뽑는다. 이 장이 나머지 7장의 기준이 된다.

```
A cute round slime creature mascot, sitting calmly and facing the viewer
with a soft neutral expression, simple clean vector shapes, soft pastel
colors, thick friendly outlines, full body visible, centered in frame
with generous empty margin on all four sides, transparent background,
no ground shadow, no text, no border, 1024x1024, flat illustration style
```

## 2. working — 누군가 작업 중

여기서부터 1번 이미지를 참조로 첨부한다.

```
Same character as the reference image, same style, same colors, same
proportions. Now leaning slightly forward, focused and absorbed in a
task, eyes narrowed in concentration. Centered, same scale and margin as
the reference, transparent background, no text, 1024x1024
```

## 3. attention-0 — 대기 발생 직후

```
Same character as the reference image, same style, same colors, same
proportions. Now tilting its head to one side with a curious expression,
looking directly at the viewer as if it just noticed something and wants
attention. Centered, same scale and margin as the reference, transparent
background, no text, 1024x1024
```

## 4. attention-1 — 2분 방치

```
Same character as the reference image, same style, same colors, same
proportions. Now raising one arm high to call out, mouth slightly open,
bright alert expression, clearly trying to get the viewer's attention.
Centered, same scale and margin as the reference, transparent background,
no text, 1024x1024
```

## 5. attention-2 — 10분 방치

```
Same character as the reference image, same style, same colors, same
proportions. Now leaning urgently toward the viewer with both arms raised,
worried and impatient expression, eyebrows raised, visibly restless.
Centered, same scale and margin as the reference, transparent background,
no text, 1024x1024
```

## 6. attention-3 — 30분 방치 (상한)

가장 강한 호소. 이 위로는 더 세지지 않는다.

```
Same character as the reference image, same style, same colors, same
proportions. Now in its most desperate pleading pose, both arms stretched
out toward the viewer, big teary pleading eyes, mouth open calling out,
begging not to be ignored. Centered, same scale and margin as the
reference, transparent background, no text, 1024x1024
```

## 7. error — 에러 발생

```
Same character as the reference image, same style, same colors, same
proportions. Now startled and alarmed, eyes wide open in shock, body
recoiling slightly backward, small sweat drop near the head. Centered,
same scale and margin as the reference, transparent background, no text,
1024x1024
```

## 8. disconnected — 연결 끊김

회색조가 핵심이다. "없다"가 아니라 "모른다"를 전달해야 한다.

```
Same character as the reference image, same pose family and proportions,
but rendered in desaturated grayscale with reduced opacity, drooping and
lifeless, eyes closed or dim, faded and ghostly as if disconnected.
Centered, same scale and margin as the reference, transparent background,
no text, 1024x1024
```

## 9. sleeping — 장시간 전체 idle (선택)

없어도 동작한다. 여유가 있으면 추가한다.

```
Same character as the reference image, same style, same colors, same
proportions. Now curled up asleep with eyes closed, peaceful expression,
a small "z z z" floating above its head. Centered, same scale and margin
as the reference, transparent background, no text, 1024x1024
```

---

## 저장 위치

```
themes/<theme-id>/
  theme.json
  assets/
    idle.png
    working.png
    attention-0.png
    attention-1.png
    attention-2.png
    attention-3.png
    error.png
    disconnected.png
    sleeping.png        # 선택
```

## 생성 후 체크리스트

- [ ] 8장이 한눈에 같은 캐릭터로 보이는가
- [ ] 배경이 실제로 투명한가 (흰색 배경이 아닌지 확인)
- [ ] 8장의 캐릭터 크기가 서로 비슷한가
- [ ] 사방 여백이 충분한가 (캐릭터가 캔버스 가장자리에 닿지 않아야 함)
- [ ] attention-0 → 1 → 2 → 3 순서로 놓았을 때 다급함이 점점 커지는 게 느껴지는가
- [ ] disconnected가 나머지와 확실히 구분되는가 (회색조)

마지막 항목이 가장 중요하다.
에스컬레이션 4장은 서로 구분되지 않으면 단계가 올라가도 사용자가 알아채지 못하고, 그러면 이 제품의 핵심 기능이 무의미해진다.
