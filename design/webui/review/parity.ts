// Parity checklist for the owner review (Russian). Status:
//   mock    — есть в мокапе, кликабельно
//   partial — в мокапе показан пример, полный объём переносится в клиент
//   native  — нужна нативная часть Tauri (Rust); в мокапе только точка входа
//   cli     — остаётся в hgs, клиент только вызывает
export type ParityStatus = "mock" | "partial" | "native" | "cli";
export interface ParityRow { what: string; where: string; status: ParityStatus; link?: string; note?: string }
export interface ParityGroup { id: string; title: string; rows: ParityRow[] }

const app = (q = "") => `./app.html${q ? `?${q}` : ""}`;

export const PARITY: ParityGroup[] = [
  { id: "window", title: "Окно и навигация", rows: [
    { what: "Боковой переключатель экранов: логотип → Overview, New session, Sessions (бейдж внимания, меню по правому клику), Projects, Machines, Accounts; внизу закрепление поверх окон, Settings, версия → About; наши иконки из WorkspaceIcons", where: "Рейл слева, как сейчас (56 px)", status: "mock", link: app() },
    { what: "Логотип Zerus окрашивается в цвет внимания и вращается, пока есть сессии, требующие внимания; стоит при «уменьшить движение» и в скрытом окне; без свечения, знак вращается целиком", where: "Рейл; варианты «Раскрутка» (по умолчанию), «Сильная раскрутка» и «как в Qt» в панели MOCK UP", status: "mock", link: app() },
    { what: "Страницы Overview, Sessions, Projects, Machines, Accounts, Settings; Ctrl+1…5, Ctrl+,", where: "Те же страницы, тот же порядок", status: "mock" },
    { what: "Статус-бар 28 px: состояние подключений, «New version», «Retry now», уведомления, «Clean up worktree…», полоса лимитов аккаунта; перед цифрами всегда тип аккаунта — агент со значком и тариф", where: "Нижняя строка, правый угол", status: "mock", link: app("demo=error") },
    { what: "Тема System/Dark/Light, масштаб контента 75–200 % (только содержимое сессии), Animate working sessions и системное «уменьшить движение», фокус-рамка только с клавиатуры", where: "Settings → Appearance + скины", status: "mock", link: app("page=settings") },
    { what: "Закрытие окна никогда не останавливает агентов; сохранение геометрии, ширины списка и панели", where: "Поведение клиента (window-state + своё хранилище)", status: "native" },
    { what: "Ширина списка сессий: перетаскивание границы (минимум 270 px, по умолчанию 310)", where: "Граница между списком и беседой", status: "mock", link: app() },
  ] },
  { id: "list", title: "Список сессий (структура и размеры как сейчас)", rows: [
    { what: "Группировка только по проектам; проект по умолчанию General; заголовки показываются при нескольких проектах; пустые проекты скрыты", where: "Список", status: "mock", link: app() },
    { what: "Заголовок проекта 40 px: полоса цвета проекта, шеврон, иконка, имя, счётчики «● работают», «● ответы», «! внимание», всего", where: "Список", status: "mock" },
    { what: "Карточка 94 px (компактная плотность — единственная, настройка плотности убрана по вашему решению): заголовок 14/600 + бейдж статуса + «новый ответ» + опциональные вопросы + агент; папка/ветка + метки Git + машина; текущее действие 12 px; модель + effort + счётчик агентов", where: "Список, геометрия из SessionCardDelegate", status: "mock" },
    { what: "Метки Git как сейчас: изменённые файлы, исходящие, входящие, «не проверено» (облако с ?), «синхронизировано» (облако с галочкой); подсказка со всеми полями", where: "Вторая строка карточки", status: "mock" },
    { what: "Вложенность: один уровень, строки субагентов 64 px с отступом 24 px; линии дерева взяты из прототипа", where: "Список (раскрыть «2/4» у api-refactor)", status: "mock" },
    { what: "Форки — обычные строки сразу после оригинала", where: "Список", status: "mock" },
    { what: "Компактная строка 40 px для отдельной сессии (двойной клик, меню, кнопка раскрытия), лимит 50 000; переход между карточкой и строкой плавный (220 мс), название не прыгает", where: "Список (scratch в General, api-refactor-fork по двойному клику)", status: "mock", link: app("select=ws:2") },
    { what: "Свёрнутая полоса 98 px с квадратными плитками 84 px, строки субагентов, «подглядывание» поверх беседы, раскрытие по наведению (150 мс, закрытие через 350 мс вне панели), тонкая полоса прокрутки только под указателем", where: "Кнопка сворачивания в шапке списка", status: "mock", link: app("strip=1") },
    { what: "Фильтры состояния: All, Needs attention, Active, Saved, Archive; фильтры машин, проектов и аккаунтов; чипы выбранного; фильтр папки", where: "Под поиском", status: "mock" },
    { what: "Поиск по метаданным, ID и содержимому сообщений (от 2 символов, по машинам), страница результатов с подсветкой", where: "Поле поиска списка", status: "mock", link: app("q=pagination") },
    { what: "Архив: страницы по 50 на машину, выбор периода, Newer/Older, Restore", where: "Фильтр Archive", status: "mock", link: app("filter=archived") },
    { what: "Строка итогов: «N shown, N working, N attention, N starting», статус создания сессий, Mark all as read, Saved drafts", where: "Низ списка", status: "mock" },
    { what: "Множественный выбор (Ctrl/Shift-клик), панель действий с количествами, подтверждение пакетных действий", where: "Список", status: "mock" },
    { what: "Перетаскивание: порядок сессий, перенос в проект, порядок проектов, перенацеливание задачи на сессию", where: "Список", status: "partial", note: "Поведение описано в спеке; в мокапе без DnD" },
    { what: "Клавиатура: ↑↓, ←→ (группы, дети, компактная строка), Enter, Esc, Menu/Shift+F10", where: "Список", status: "mock" },
    { what: "Пустой список, загрузка, «No matching sessions», офлайн машины", where: "Переключатель состояний в панели MOCKUP", status: "mock", link: app("demo=empty") },
    { what: "Виртуализация: 20 000 сессий без потери плавности", where: "Кнопка «20k» в панели MOCKUP", status: "mock" },
  ] },
  { id: "status", title: "Статусы и признаки", rows: [
    { what: "Полный порядок статусов: Offline, Archived, Paused, Stopped, Agent exited, Not tracked, Needs approval, Sign in, Needs input, ошибки провайдера, Retry in N s, Retrying, Error, Interrupted, Compacting, Starting, Using a tool, Working, Background, Waiting for result, Scheduled · HH:mm, Ready", where: "Бейджи карточки и шапки", status: "partial", note: "В мокапе показаны основные; полный порядок — данные из hgs без изменений" },
    { what: "Приоритет бейджа: внимание/ошибка → работа/фон → черновик → новый ответ → по расписанию → ожидание → пауза → нейтральный", where: "Карточка", status: "mock" },
    { what: "Метка края 4 px (в Qt 3 px): внимание, ошибка, непрочитанное, черновик, без звука; 6 px от краёв карточки, 4 px в компактной строке", where: "Карточка и компактная строка", status: "mock" },
    { what: "Background = простаивает, но есть свои живые задачи; остаётся в Active и принимает сообщения", where: "cursor-bench", status: "mock" },
    { what: "Scheduled между Working и Ready, никогда не Needs attention", where: "nightly-report, dns-rotation", status: "mock" },
    { what: "Без звука (общая настройка хоста), «напомнить позже», опциональные вопросы не считаются вниманием", where: "installer-sign, style-guide, docs-sync", status: "mock" },
    { what: "Отметка прочитанного после 1 с видимости ответа; синхронизация с хостами", where: "Поведение клиента", status: "native" },
  ] },
  { id: "detail", title: "Беседа: шапка, вкладки, Activity", rows: [
    { what: "Шапка: «Проект / тег», индикатор удалённого чтения, статус, кнопки папки, shell, копирования ID, очистки, Restore, меню ⋯; вторая строка: агент, машина, ветка Git плашкой со значком (если есть), папка, worktree, модель", where: "Над вкладками", status: "mock" },
    { what: "Кнопка цели (Pursuing goal …) → Plan & agents", where: "Под шапкой", status: "mock" },
    { what: "Вкладки Activity, Terminal, Native UI (DeepSeek), Processes (N), Task View; переключатель боковой панели", where: "Беседа", status: "mock" },
    { what: "Колонка чтения 900 px (480–2400), перетаскивание краёв (оба края вместе), двойной клик — сброс, Full-width Activity и Reset Activity width в меню", where: "Края колонки Activity; вариант раскладки скина", status: "mock", link: app() },
    { what: "Сообщения: ваши — буквальный текст, агента — Markdown, Copy as Markdown, квитанции доставки, группы инструментов, отчёт субагента, мысли, граница очистки сессии", where: "Activity", status: "mock" },
    { what: "Голосовое сообщение: плеер с волной и кнопка транскрипта справа; видео-«кружок» 200 px проигрывается на месте (кольцо прогресса, время), кнопка транскрипта справа; значок состояния доставки", where: "Activity, ниже по беседе api-refactor", status: "mock", link: app("#item-a3d") },
    { what: "Вложения (миниатюры, Open), ссылки и ссылки на файлы с диалогом открытия/скачивания", where: "Activity", status: "partial" },
    { what: "Следование за последним, Jump to latest, «Queued in agent» с Send now, режим результата поиска", where: "Activity", status: "mock" },
    { what: "Вид субагента: «← Main activity», только чтение или отправка напрямую", where: "Клик по строке субагента", status: "mock" },
    { what: "Баннеры: вход в DeepSeek, остановленный OpenCode", where: "Activity", status: "partial" },
    { what: "Пустые состояния: «A little more focus.», «Ready for your first message» и др.", where: "Беседа", status: "mock", link: app("select=none") },
  ] },
  { id: "questions", title: "Вопросы и подтверждения агента", rows: [
    { what: "Очередь: обязательные раньше опциональных, навигация по опциональным, черновики ответов", where: "Карточка вопроса в Activity", status: "partial" },
    { what: "Один/несколько вариантов, Other, обратный отсчёт 3 с, Approve once / Deny, Confirm для расширения прав, Skip", where: "Карточка вопроса (checkout-ui, migrate-db)", status: "mock", link: app("select=b1:1") },
    { what: "Причины недоступности (машина офлайн, сессия не запущена и т. д.)", where: "Карточка вопроса", status: "partial" },
  ] },
  { id: "composer", title: "Поле ввода", rows: [
    { what: "Панель 34 px: вложения, запись голоса или видео и переключатель режима (виден до записи, заблокирован во время неё), Latest, Mark as read, сжатие; справа лимит, восстановление, кэш, контекст", where: "Над полем ввода", status: "mock" },
    { what: "Изменение высоты поля 76–720 px: видимая ручка над полем, двойной клик и Home возвращают 76 px, стрелки по 20 px; высота запоминается", where: "Ручка над полем ввода", status: "mock" },
    { what: "Enter — отправить, Shift+Enter — строка, Esc — прервать, Tab — подсказка агента", where: "Поле ввода", status: "partial" },
    { what: "Вложения: до 8 файлов, 10 МиБ на файл, 20 МиБ на сообщение; вставка картинок; перетаскивание; ссылка на существующий файл", where: "Поле ввода", status: "partial" },
    { what: "Модель и effort, Stop, Send, Send and push now, диалоги холодного кэша и остановленной сессии, причины блокировки отправки", where: "Поле ввода", status: "partial" },
    { what: "Черновики на устройстве, переживают падения и завершённые сессии; окно Saved drafts", where: "Поле ввода, Saved drafts", status: "mock", link: app("dialog=drafts") },
  ] },
  { id: "tabs", title: "Terminal, Processes, Native UI, боковая панель", rows: [
    { what: "Terminal: живой терминал сессии (xterm.js), Reconnect/Disconnect, прокрутка 5000 строк, копирование и вставка", where: "Вкладка Terminal", status: "mock", link: app("tab=terminal"), note: "Рендер — xterm.js, PTY — Rust (portable-pty)" },
    { what: "Processes: живые/записанные/завершённые, Stop selected (TERM → KILL через 2 с), вывод", where: "Вкладка Processes", status: "mock", link: app("tab=processes") },
    { what: "Native UI DeepSeek: отдельный webview или системный браузер", where: "Вкладка Native UI", status: "native", link: app("select=b1:1&tab=native") },
    { what: "Боковая панель: Plan & agents (цель, задачи плана, субагенты), Details (ID, аккаунт, папка, использование токенов), Worktrees (дерево, фильтр по папке, Clean up), Tasks; в стиле карточек прототипа (блоки со сворачиванием, появление и скрытие с анимацией), содержимое наше", where: "Справа, 400 px (ширина перетаскивается от 240, запоминается), Ctrl+I", status: "mock", link: app("itab=details") },
  ] },
  { id: "tasks", title: "Задачи (редизайн без оркестратора форка)", rows: [
    { what: "Стадии Review, In progress, To do, Backlog, Done; карточки задач; фильтр стадий", where: "Боковая панель → Tasks", status: "mock", link: app("itab=tasks") },
    { what: "Task View: детали, история, комментарии, цель (сессия, любая, новая сессия, субагент), вложения, отправка и отчёт", where: "Вкладка Task View", status: "mock", link: app("itab=tasks&tab=taskview") },
    { what: "Неуверенная доставка — только ручной разбор, никогда не повторяется автоматически; причины недоступности действий", where: "Task View", status: "mock" },
    { what: "Диалоги задач (редактирование, отмена с «Keep running» по умолчанию, настройки проекта, освобождение записи восстановления)", where: "Диалоги", status: "mock" },
    { what: "Не переносим: команды, лиды, фазы, граф, слои, iron rules, удержание кэша, автоприём, дизайнер", where: "—", status: "mock", note: "Сознательно отсутствуют" },
  ] },
  { id: "pages", title: "Overview, Projects, New session, Machines, Accounts", rows: [
    { what: "Overview: плитки (внимание, работают, машины онлайн, сессии), карточки машин, аккаунты, «Attention & in progress»", where: "Логотип в рейле", status: "mock", link: app("page=dashboard") },
    { what: "Projects: проекты, цвета, папки, Clone repository, Worktrees, Swarm…", where: "Projects", status: "mock", link: app("page=projects") },
    { what: "Очистка worktree: просмотр, согласия, исходы «удалено/оставлено/неизвестно», ветки и игнорируемые данные сохраняются", where: "Worktrees → Clean up", status: "mock", link: app("dialog=worktree-cleanup") },
    { what: "New session: машина, агент, аккаунт, папка, worktree (создаётся только после Start), прямой запуск, своя/существующая worktree, модель, предупреждения", where: "Кнопка «+» в рейле, Ctrl+N", status: "mock", link: app("dialog=new-session") },
    { what: "Machines: статус подключения, платформа, версия, CPU/память, цвет, SSH, добавление/изменение/удаление", where: "Machines", status: "mock", link: app("page=machines") },
    { what: "Accounts: аккаунты по машинам, лимиты 5h/7d, вход, установка, профили", where: "Accounts", status: "mock", link: app("page=accounts") },
  ] },
  { id: "settings", title: "Settings", rows: [
    { what: "Appearance (скины, тема, масштаб, анимации, раскрытие по наведению, индикатор лимитов, язык), Sessions, Automatic recovery, Processes, Updates, Mobile connection, Files, Worktrees, Automatic approvals (опасная опция, выключена), Voice input, Keyboard, Private analytics, About", where: "Settings", status: "mock", link: app("page=settings") },
    { what: "Скины: выбор, импорт JSON (только токены и варианты), папка скинов", where: "Settings → Appearance", status: "mock" },
    { what: "Сочетания клавиш: 28 команд, до 2 сочетаний, отметка «работает в Terminal», конфликты, сброс", where: "Settings → Keyboard, F1", status: "mock", link: app("dialog=shortcuts") },
    { what: "Swarm, телефоны, relay, сопряжение по QR", where: "Settings → Mobile connection, Projects → Swarm", status: "mock" },
  ] },
  { id: "menus", title: "Контекстные меню", rows: [
    { what: "Карточка сессии: все пункты (компактная строка, терминал, папка, shell, команда подключения, звук, прочитано/напомнить, пауза/возобновление, очистка, переименование, форк, архив, проект, копирование, завершить/забыть)", where: "Правый клик по карточке", status: "mock" },
    { what: "Заголовок проекта, фон списка, рейл Sessions, «⋯ Session and machine actions», статус создания, множественный выбор", where: "Список", status: "mock" },
    { what: "Activity (Full-width Activity, Reset width), поле ввода (Files), Terminal (Copy/Paste), Processes (Terminate), Worktrees (Remove/Copy path)", where: "Соответствующие области", status: "partial" },
  ] },
  { id: "native", title: "Интеграции с системой (нужна нативная работа в Tauri)", rows: [
    { what: "Иконка в трее с двумя счётчиками, меню с машинами и сессиями, «Open in ▸»", where: "Rust: tauri tray + menu", status: "native", note: "Linux: левый клик через appindicator не приходит — нужен свой StatusNotifierItem (ksni/zbus)" },
    { what: "Уведомления с токеном конкретной беседы, отзыв решённых, действия на Linux, показ на переднем плане на macOS", where: "Rust: свой модуль (zbus / UserNotifications / WinRT)", status: "native", note: "Официальный плагин этого не покрывает" },
    { what: "Бейдж в доке/панели задач", where: "Tauri set_badge_count / overlay icon; Unity LauncherEntry", status: "native" },
    { what: "Присутствие: простой и блокировка для уведомлений на телефон", where: "Rust: CoreGraphics / zbus / x11rb; запись через hgs presence", status: "native" },
    { what: "Поверх всех окон: X11/macOS/Windows встроенно; KDE Wayland — скрипты KWin", where: "Rust: set_always_on_top + zbus KWin", status: "native" },
    { what: "Открытие терминала: Konsole (D-Bus), xdg-terminal-exec, Terminal.app, Windows Terminal/консоль", where: "Rust-модуль терминалов", status: "native" },
    { what: "Встроенный терминал: PTY (forkpty / ConPTY + Job Object)", where: "Rust: portable-pty → xterm.js по каналу", status: "native" },
    { what: "Запись голоса и видео: разрешения, форматы, лимиты", where: "Нативный захват в Rust (cpal) как основной путь; getUserMedia — после замера", status: "native" },
    { what: "Сторожевой таймер главного потока, перезапуск сервисом, сигналы завершения, перезапуск после падения на Windows", where: "Rust-поток + systemd/launchd", status: "native" },
    { what: "Один экземпляр с передачей сервисной копии, автозапуск через сервисы установщика", where: "single-instance + своя логика", status: "native" },
    { what: "Обновление: каталог, «Restart to finish update», update-installed.py, пакетные менеджеры", where: "Своя логика каталога в Rust, process restart", status: "native" },
    { what: "Буфер обмена (Klipper/wl-copy на Wayland), отметки скачанных файлов (quarantine / Zone.Identifier), кэш загрузок, выбор файлов", where: "clipboard-manager, opener, dialog + Rust", status: "native" },
    { what: "Язык интерфейса (сейчас русский перевод Qt)", where: "i18n во фронтенде + строки меню трея в Rust", status: "native" },
    { what: "Без звука, прочитанное, автоподтверждения, worktree-политики, swarm, речь — данные и правила", where: "hgs CLI", status: "cli" },
  ] },
];
