#include <QApplication>
#include <QImage>
#include <QImageReader>
#include <QtTest>
#include <QScopeGuard>
#include "IconFactory.h"

// Легитимность цифры на глаз не проверить (см. план задачи), поэтому тест бьёт по
// нефизическим инвариантам: countText() — чистая функция, а QIcon обязан нести все
// заявленные размеры квадратными пиксмапами и не быть пустым ни в одном состоянии,
// включая forCount(0, ...) — ноль сессий это нормальное состояние, а не повод спрятать
// иконку (пустая иконка в трее выглядит как упавшее приложение).
class TestIconFactory : public QObject {
    Q_OBJECT
private slots:
    void countTextSmallNumbers();
    void countTextCapsAtNinetyNinePlus();
    void countTextClampsNegative();
    void forCountIsNeverNull();
    void forCountHasAllFiveSizes();
    void forCountPixmapsAreSquare();
    void forCountZeroIsStillVisible();
    void forErrorIsNeverNull();
    void foregroundFallsBackToPaletteOnEmptyOrInvalidOverride();
    void foregroundParsesValidOverride();
    void forCountIsMemoisedPerLook();
    void memoDoesNotSurviveColourChange();
    void forErrorIsMemoised();
    void brandSurvivesCounterAndError();
    void attentionHighlightSurvivesPlatformAndOverride();
    void svgResourcesRender();
};

void TestIconFactory::svgResourcesRender()
{
    // Loading a QIcon alone does not prove that its image-format plugin exists.
    // The packaged desktop uses these SVG resources for menu/combo arrows.
    for (const auto *path : {":/hgs/chevron-down.svg", ":/hgs/chevron-up.svg", ":/hgs/chevron-right.svg", ":/hgs/icons/hgs-zerus-symbolic.svg"}) {
        QImageReader reader(QString::fromLatin1(path));
        const QImage image = reader.read();
        QVERIFY2(!image.isNull(), qPrintable(QString::fromLatin1(path) + ": " + reader.errorString()));
    }
}

void TestIconFactory::countTextSmallNumbers()
{
    QCOMPARE(IconFactory::countText(0), QStringLiteral("0"));
    QCOMPARE(IconFactory::countText(1), QStringLiteral("1"));
    QCOMPARE(IconFactory::countText(9), QStringLiteral("9"));
    QCOMPARE(IconFactory::countText(42), QStringLiteral("42"));
    QCOMPARE(IconFactory::countText(99), QStringLiteral("99"));
}

void TestIconFactory::countTextCapsAtNinetyNinePlus()
{
    // Обрезаем на уровне СТРОКИ (не подбором кегля): "99+" несёт то же сообщение
    // ("много"), что и точное трёхзначное число, но не заставляет renderTile() душить
    // шрифт до нечитаемости на 22px клетке.
    QCOMPARE(IconFactory::countText(100), QStringLiteral("99+"));
    QCOMPARE(IconFactory::countText(101), QStringLiteral("99+"));
    QCOMPARE(IconFactory::countText(9999), QStringLiteral("99+"));
}

void TestIconFactory::countTextClampsNegative()
{
    // Отрицательной суммы сессий по флоту не бывает, но защитный путь не должен
    // печатать минус на плитке, где для него нет места.
    QCOMPARE(IconFactory::countText(-1), QStringLiteral("0"));
    QCOMPARE(IconFactory::countText(-100), QStringLiteral("0"));
}

void TestIconFactory::forCountIsNeverNull()
{
    IconFactory factory;
    QVERIFY(!factory.forCounts(0, 0, false).isNull());
    QVERIFY(!factory.forCounts(7, 0, false).isNull());
    QVERIFY(!factory.forCounts(150, 0, true).isNull());
}

void TestIconFactory::forCountHasAllFiveSizes()
{
    IconFactory factory;
    const QIcon icon = factory.forCounts(3, 0, false);
    const QList<QSize> sizes = icon.availableSizes();
    for (int expected : {16, 22, 24, 32, 48})
        QVERIFY2(sizes.contains(QSize(expected, expected)),
                  qPrintable(QStringLiteral("missing size %1").arg(expected)));
    QCOMPARE(sizes.size(), 5);
}

void TestIconFactory::forCountPixmapsAreSquare()
{
    IconFactory factory;
    const QIcon icon = factory.forCounts(12, 0, false);
    for (int size : {16, 22, 24, 32, 48}) {
        const QPixmap pm = icon.pixmap(QSize(size, size));
        QVERIFY(!pm.isNull());
        QCOMPARE(pm.width(), size);
        QCOMPARE(pm.height(), size);
    }
}

void TestIconFactory::forCountZeroIsStillVisible()
{
    // "0" рисуется как обычная цифра, а не пустая/затемнённая плитка: ноль сессий —
    // нормальное состояние флота, а невидимая иконка в трее читается как "трей упал".
    IconFactory factory;
    const QIcon icon = factory.forCounts(0, 0, false);
    const QImage img = icon.pixmap(QSize(22, 22)).toImage();
    bool sawOpaquePixel = false;
    for (int y = 0; y < img.height() && !sawOpaquePixel; ++y) {
        for (int x = 0; x < img.width(); ++x) {
            if (qAlpha(img.pixel(x, y)) > 0) {
                sawOpaquePixel = true;
                break;
            }
        }
    }
    QVERIFY(sawOpaquePixel);
}

void TestIconFactory::forErrorIsNeverNull()
{
    IconFactory factory;
    const QIcon icon = factory.forError();
    QVERIFY(!icon.isNull());
    QCOMPARE(icon.availableSizes().size(), 5);
}

void TestIconFactory::foregroundFallsBackToPaletteOnEmptyOrInvalidOverride()
{
    // The test owns what the fallback reads, so the machine's theme cannot
    // decide it: on Windows the taskbar's theme, elsewhere the palette.
#ifdef Q_OS_WIN
    const auto restore = qScopeGuard([] { IconFactory::setTaskbarLightForTests(-1); });
    for (const auto &[light, expected] : {std::pair{1, QColor(Qt::black)}, std::pair{0, QColor(Qt::white)}}) {
        IconFactory::setTaskbarLightForTests(light);
        QCOMPARE(IconFactory::foreground(QString()), expected);
        QCOMPARE(IconFactory::foreground(QStringLiteral("not-a-color")), expected);
    }
#else
    const QPalette before = QApplication::palette();
    const auto restore = qScopeGuard([before] { QApplication::setPalette(before); });
    for (const QColor text : {QColor(0x12, 0x34, 0x56), QColor(0xee, 0xdd, 0xcc)}) {
        QPalette palette = before; palette.setColor(QPalette::WindowText, text);
        QApplication::setPalette(palette);
        QCOMPARE(IconFactory::foreground(QString()), text);
        QCOMPARE(IconFactory::foreground(QStringLiteral("not-a-color")), text);
    }
#endif
}

void TestIconFactory::foregroundParsesValidOverride()
{
    QCOMPARE(IconFactory::foreground(QStringLiteral("#ff0000")), QColor(0xff, 0, 0));
}

void TestIconFactory::forCountIsMemoisedPerLook()
{
    // Один и тот же вид -> тот же QIcon (равный cacheKey). По нему QuietTrayIcon отличает
    // «ничего не поменялось» от «перерисовать» (см. QuietTrayIcon.h): новый объект на
    // каждый тик означал бы новый cacheKey и обновление NSStatusItem каждые 2 с.
    IconFactory factory;
    QCOMPARE(factory.forCounts(3, 0, false).cacheKey(), factory.forCounts(3, 0, false).cacheKey());
    QVERIFY(factory.forCounts(3, 0, false).cacheKey() != factory.forCounts(4, 0, false).cacheKey());
    QVERIFY(factory.forCounts(3, 0, false).cacheKey() != factory.forCounts(3, 0, true).cacheKey());
    QVERIFY(factory.forCounts(3, 0, false).cacheKey() != factory.forCounts(3, 1, false).cacheKey());
}

void TestIconFactory::memoDoesNotSurviveColourChange()
{
    // Цвет цифры -- часть вида: смена палитры (тёмная/светлая тема) или override
    // обязана дать новую плитку, а не старую из кэша.
    IconFactory factory;
    const qint64 before = factory.forCounts(3, 0, false).cacheKey();
    factory.setForegroundOverride(QStringLiteral("#ff0000"));
    QVERIFY(factory.forCounts(3, 0, false).cacheKey() != before);
}

void TestIconFactory::forErrorIsMemoised()
{
    IconFactory factory;
    QCOMPARE(factory.forError().cacheKey(), factory.forError().cacheKey());
}

void TestIconFactory::brandSurvivesCounterAndError()
{
    IconFactory factory;
    // Updating attention must preserve the total and Swarm in the top row.
    for (int size : {16, 22, 24, 32, 48}) {
        const auto upper = factory.forCounts(12, 0, false).pixmap(size, size).toImage().copy(0, 0, size, size / 2);
        for (int attention : {1, 9, 12})
            QCOMPARE(factory.forCounts(12, attention, false).pixmap(size, size).toImage().copy(0, 0, size, size / 2), upper);
    }
}

void TestIconFactory::attentionHighlightSurvivesPlatformAndOverride()
{
    IconFactory factory;
    for (const QString &override : {QString(), QString("#f0f0f0"), QString("#202020")}) {
        factory.setForegroundOverride(override);
        // macOS must preserve the amber surface instead of using a template mask.
        QVERIFY(!factory.forCounts(12, 2, false).isMask());
        const auto active = factory.forCounts(12, 2, false).pixmap(22, 22).toImage();
        const auto quiet = factory.forCounts(12, 0, false).pixmap(22, 22).toImage();
        QVERIFY(active != quiet);
        int amber = 0;
        for (int y = 11; y < 22; ++y)
            for (int x = 0; x < 22; ++x) {
                const QColor pixel = active.pixelColor(x, y);
                if (pixel.alpha() > 200 && pixel.red() > 220 && pixel.green() > 150 && pixel.blue() < 130) ++amber;
            }
        QVERIFY(amber > 60);
    }
}

QTEST_MAIN(TestIconFactory)
#include "test_iconfactory.moc"
