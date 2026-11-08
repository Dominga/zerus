// Task 111: hgs withdraws a proposal that can no longer apply (its phase
// closed, its task was decided, its member left) and refuses an Allow on it in
// these words. Such a card is no longer the owner's to answer: it leaves at
// once, like a confirmed one, and the lead is told why.
export const withdrawnRefusal = (detail: string): boolean => /can no longer apply/.test(detail) && /withdrawn/.test(detail);
