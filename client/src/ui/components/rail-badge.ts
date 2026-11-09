/** The Sessions badge's text, capped as Qt's (SessionsWindow.cpp:3378): the exact count stays in the button's name. */
export const attentionBadgeText = (count: number) => (count > 99 ? "99+" : String(count));
