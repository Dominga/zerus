#pragma once
#include <QAudioOutput>
#include <QBoxLayout>
#include <QDesktopServices>
#include <QDialog>
#include <QFileInfo>
#include <QImageReader>
#include <QLabel>
#include <QMediaPlayer>
#include <QMovie>
#include <QPainter>
#include <QPointer>
#include <QPushButton>
#include <QSlider>
#include <QTimer>
#include <QUrl>
#include <QVideoFrame>
#include <QVideoSink>
#include <functional>

// WhiteWh fork: pictures, animations and videos an agent names in its messages,
// shown in Activity (ActivityView) and opened here. Only Qt Multimedia is used:
// a video sink paints the frames, as the round video messages do.
namespace MediaFiles {
enum class Kind { None, Image, Animation, Video };

inline Kind kind(const QString &path)
{
    const auto suffix = QFileInfo(path).suffix().toLower();
    if (QStringList{"png", "jpg", "jpeg", "webp", "bmp"}.contains(suffix)) return Kind::Image;
    if (suffix == "gif") return Kind::Animation;
    if (QStringList{"mp4", "webm", "mov", "mkv", "m4v", "avi"}.contains(suffix)) return Kind::Video;
    return Kind::None;
}

// Files larger than this are not read for a preview; they still open.
constexpr qint64 PreviewLimit = 64LL * 1024 * 1024;

// A picture no larger than `bound`, or a null image.
inline QImage picture(const QString &path, const QSize &bound)
{
    QImageReader reader(path);
    reader.setAutoTransform(true);
    const auto size = reader.size();
    if (size.isValid() && qint64(size.width()) * size.height() > 64LL * 1000 * 1000) return {};
    if (size.isValid() && (size.width() > bound.width() || size.height() > bound.height()))
        reader.setScaledSize(size.scaled(bound, Qt::KeepAspectRatio));
    return reader.read();
}

// The first frame of a video, handed to `done` (null when none arrives within 8 seconds).
inline void poster(const QString &path, QObject *context, std::function<void(const QImage &)> done)
{
    auto *player = new QMediaPlayer(context);
    auto *sink = new QVideoSink(player);
    player->setVideoSink(sink);
    auto finished = std::make_shared<bool>(false);
    const auto finish = [player, finished, done](const QImage &image) {
        if (*finished) return;
        *finished = true;
        player->stop();
        player->deleteLater();
        done(image);
    };
    QObject::connect(sink, &QVideoSink::videoFrameChanged, player, [finish](const QVideoFrame &frame) {
        if (frame.isValid()) finish(frame.toImage());
    });
    QObject::connect(player, &QMediaPlayer::errorOccurred, player, [finish] { finish({}); });
    QTimer::singleShot(8000, player, [finish] { finish({}); });
    player->setSource(QUrl::fromLocalFile(path));
    player->play();
}
} // namespace MediaFiles

// A widget that paints the frames of a video sink, fitted and centred.
class MediaVideoView : public QWidget {
public:
    explicit MediaVideoView(QWidget *parent = nullptr) : QWidget(parent), m_sink(new QVideoSink(this))
    {
        setMinimumSize(320, 180);
        QObject::connect(m_sink, &QVideoSink::videoFrameChanged, this, [this](const QVideoFrame &frame) {
            m_frame = frame.toImage();
            update();
        });
    }
    QVideoSink *sink() const { return m_sink; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.fillRect(rect(), Qt::black);
        if (m_frame.isNull()) return;
        const auto size = m_frame.size().scaled(this->size(), Qt::KeepAspectRatio);
        p.setRenderHint(QPainter::SmoothPixmapTransform);
        p.drawImage(QRect(QPoint((width() - size.width()) / 2, (height() - size.height()) / 2), size), m_frame);
    }

private:
    QVideoSink *m_sink;
    QImage m_frame;
};

// The viewer: a picture or animation at up to the screen's size, or a player.
class MediaViewer : public QDialog {
public:
    MediaViewer(const QString &path, const QString &title, QWidget *parent = nullptr) : QDialog(parent), m_path(path)
    {
        setObjectName("mediaViewer");
        setWindowTitle(title);
        setAttribute(Qt::WA_DeleteOnClose);
        auto *layout = new QVBoxLayout(this);
        layout->setContentsMargins(12, 12, 12, 12);
        layout->setSpacing(10);
        const auto kind = MediaFiles::kind(path);
        const QSize bound(1280, 860);
        if (kind == MediaFiles::Kind::Video) {
            auto *view = new MediaVideoView;
            view->setObjectName("mediaVideo");
            view->resize(960, 540);
            view->setMinimumSize(640, 360);
            layout->addWidget(view, 1);
            m_player = new QMediaPlayer(this);
            m_output = new QAudioOutput(this);
            m_player->setAudioOutput(m_output);
            m_player->setVideoOutput(view->sink());
            auto *row = new QHBoxLayout;
            auto *play = new QPushButton(tr("Pause"));
            play->setObjectName("mediaPlay");
            auto *position = new QSlider(Qt::Horizontal);
            position->setObjectName("mediaPosition");
            auto *time = new QLabel;
            time->setObjectName("mediaTime");
            row->addWidget(play);
            row->addWidget(position, 1);
            row->addWidget(time);
            layout->addLayout(row);
            QObject::connect(play, &QPushButton::clicked, this, [this] {
                m_player->playbackState() == QMediaPlayer::PlayingState ? m_player->pause() : m_player->play();
            });
            QObject::connect(m_player, &QMediaPlayer::playbackStateChanged, this, [play](QMediaPlayer::PlaybackState state) {
                play->setText(state == QMediaPlayer::PlayingState ? tr("Pause") : tr("Play"));
            });
            QObject::connect(m_player, &QMediaPlayer::durationChanged, this, [position](qint64 duration) { position->setRange(0, int(duration)); });
            QObject::connect(m_player, &QMediaPlayer::positionChanged, this, [this, position, time](qint64 at) {
                if (!position->isSliderDown()) position->setValue(int(at));
                time->setText(clock(at) + " / " + clock(m_player->duration()));
            });
            QObject::connect(position, &QSlider::sliderMoved, m_player, [this](int at) { m_player->setPosition(at); });
            QObject::connect(m_player, &QMediaPlayer::errorOccurred, this, [time](QMediaPlayer::Error, const QString &error) { time->setText(error); });
            m_player->setSource(QUrl::fromLocalFile(path));
            m_player->play();
        } else {
            auto *label = new QLabel;
            label->setObjectName("mediaPicture");
            label->setAlignment(Qt::AlignCenter);
            if (kind == MediaFiles::Kind::Animation) {
                auto *movie = new QMovie(path, {}, label);
                movie->jumpToFrame(0);
                const auto size = movie->currentImage().size();
                if (size.isValid() && (size.width() > bound.width() || size.height() > bound.height()))
                    movie->setScaledSize(size.scaled(bound, Qt::KeepAspectRatio));
                label->setMovie(movie);
                movie->start();
            } else {
                const auto image = MediaFiles::picture(path, bound);
                if (image.isNull()) label->setText(tr("This picture cannot be shown."));
                else label->setPixmap(QPixmap::fromImage(image));
            }
            layout->addWidget(label, 1);
        }
        auto *buttons = new QHBoxLayout;
        buttons->addStretch(1);
        auto *external = new QPushButton(tr("Open in default app"));
        external->setObjectName("mediaOpenExternal");
        auto *close = new QPushButton(tr("Close"));
        buttons->addWidget(external);
        buttons->addWidget(close);
        layout->addLayout(buttons);
        QObject::connect(external, &QPushButton::clicked, this, [this] { QDesktopServices::openUrl(QUrl::fromLocalFile(m_path)); });
        QObject::connect(close, &QPushButton::clicked, this, &QDialog::close);
    }

    static QString clock(qint64 milliseconds)
    {
        const qint64 seconds = qMax<qint64>(0, milliseconds / 1000);
        return QStringLiteral("%1:%2").arg(seconds / 60).arg(seconds % 60, 2, 10, QChar('0'));
    }

protected:
    void hideEvent(QHideEvent *event) override
    {
        if (m_player) m_player->stop();
        QDialog::hideEvent(event);
    }

private:
    QString m_path;
    QMediaPlayer *m_player = nullptr;
    QAudioOutput *m_output = nullptr;
};
