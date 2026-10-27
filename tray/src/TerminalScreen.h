#pragma once

#include <QAbstractScrollArea>
#include <QTimer>
#include <deque>
#include <vterm.h>

// Qt paints cells; libvterm owns terminal parsing, modes and input encoding.
class TerminalScreen final : public QAbstractScrollArea {
  Q_OBJECT
public:
  explicit TerminalScreen(QWidget *parent = nullptr);
  ~TerminalScreen() override;
  void feed(const QByteArray &bytes);
  void reset();
  void pasteText(const QString &text);
  QSize terminalSize() const { return QSize(m_columns, m_rows); }
  QString screenText() const;
  QString selectedText() const;
  int scrollbackLines() const { return static_cast<int>(m_history.size()); }
  VTermScreenCell screenCell(int row, int column) const;
  // The colour around the rounded box; invalid keeps square corners.
  void setCornerColor(const QColor &color);
  static constexpr int CornerRadius = 10;

signals:
  void output(const QByteArray &bytes);
  void sizeChanged(QSize cells);
  void titleChanged(const QString &title);
  void bell();

protected:
  bool event(QEvent *event) override;
  void paintEvent(QPaintEvent *) override;
  void resizeEvent(QResizeEvent *) override;
  void showEvent(QShowEvent *) override;
  void keyPressEvent(QKeyEvent *) override;
  void mousePressEvent(QMouseEvent *) override;
  void mouseMoveEvent(QMouseEvent *) override;
  void mouseReleaseEvent(QMouseEvent *) override;
  void mouseDoubleClickEvent(QMouseEvent *) override;
  void wheelEvent(QWheelEvent *) override;
  void contextMenuEvent(QContextMenuEvent *) override;
  void focusInEvent(QFocusEvent *) override;
  void focusOutEvent(QFocusEvent *) override;
  void inputMethodEvent(QInputMethodEvent *) override;
  QVariant inputMethodQuery(Qt::InputMethodQuery query) const override;

private:
  using Line = QVector<VTermScreenCell>;
  static int damaged(VTermRect, void *);
  static int moved(VTermRect, VTermRect, void *);
  static int cursorMoved(VTermPos, VTermPos, int, void *);
  static int propertyChanged(VTermProp, VTermValue *, void *);
  static int ring(void *);
  static int resized(int, int, void *);
  static int pushLine(int, const VTermScreenCell *, void *);
  static int popLine(int, VTermScreenCell *, void *);
  static int clearHistory(void *);
  static void produced(const char *, size_t, void *);
  void refreshMetrics();
  void updateGrid();
  void updateScrollBar(bool wasAtBottom);
  void scrollToBottom();
  void updateCells(VTermRect cells);
  void updateCursor();
  void copySelection();
  void pasteClipboard();
  VTermScreenCell cell(int absoluteRow, int column) const;
  QPoint position(const QPointF &point) const;
  bool isSelected(int row, int column) const;
  void mouseInput(QMouseEvent *event, bool pressed);
  QColor color(VTermColor value) const;
  static QString characters(const VTermScreenCell &cell);
  static VTermModifier modifiers(Qt::KeyboardModifiers mods);

  VTerm *m_terminal = nullptr;
  VTermScreen *m_screen = nullptr;
  VTermState *m_state = nullptr;
  int m_rows = 24, m_columns = 80;
  int m_cellWidth = 9, m_cellHeight = 18, m_ascent = 14;
  VTermPos m_cursor{0, 0};
  bool m_cursorVisible = true, m_cursorBlink = true, m_blinkOn = true;
  bool m_alternate = false, m_reverse = false;
  int m_cursorShape = VTERM_PROP_CURSORSHAPE_BLOCK;
  int m_mouseMode = VTERM_PROP_MOUSE_NONE;
  int m_wheelDelta = 0;
  QByteArray *m_batchedOutput = nullptr;
  std::deque<Line> m_history;
  qsizetype m_historyBytes = 0;
  QPoint m_selectionStart{-1, -1}, m_selectionEnd{-1, -1};
  bool m_selecting = false;
  QColor m_cornerColor;
  QString m_title, m_preedit;
  QTimer m_blink;
};
