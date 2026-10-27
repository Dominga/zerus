#pragma once

#include <QList>
#include <QMap>
#include <QString>

#include "HgsClient.h"
#include "ReadMarks.h"

// Состояние флота: свой бокс плюс пиры, каждый со временем последнего успешного опроса.
//
// Правило счёта: спящий ноут — это ПАУЗА, а не смерть. Его сессии живы и продолжают
// считаться; несвежесть данных показывается точкой у цифры, а не вычитанием из неё.
// Врать в меньшую сторону так же плохо, как в большую.
//
// Это распространяется и на переход ok:true -> ok:false: когда пир засыпает, hgs
// возвращает ok:false с пустым sessions (спросить уже некого), но FleetState не даёт
// этому затереть последний известный список — иначе цифра просела бы ровно в момент,
// когда крышку ноута закрыли, что и есть запрещённая ложь вниз. См. FleetState.cpp.
class FleetState {
public:
    // static const qint64 в заголовке не ODR-используется большинством выражений, но
    // QCOMPARE/QVERIFY в тестах берут его по ссылке — без определения это падает на
    // линковке. constexpr в C++17 неявно inline для static-членов класса, поэтому
    // определение есть в каждой единице трансляции и линковщик не жалуется. Простой
    // static const этого не даёт (нужно было бы отдельное определение в .cpp).
    static constexpr qint64 kPeerStaleMs = 180000;   // 3 минуты без ответа -> данные несвежие

    void setLocal(const BoxState &box, qint64 nowMs);
    void registerPeer(const QString &alias);     // объявить пир до первого ответа
    void setPeer(const BoxState &box, qint64 nowMs);
    // A listing whose digest (BoxState::digest) equals that of the listing
    // this host's box came from: only its observation fields are taken, the
    // poll time, machine metrics and process sample times. Read state is
    // kept. Returns false and changes nothing otherwise; setLocal/setPeer
    // then apply it in full. host is empty for this computer.
    bool refreshListing(const QString &host, const BoxState &box, qint64 nowMs);
    // Takes those observation fields from source, a copy of this fleet whose
    // listings have only been refreshed since; this state's read marks stay.
    // Returns false and changes nothing when any listing differs.
    bool refreshObservations(const FleetState &source);
    // Which peers count as reachable by poll age at nowMs (kPeerStaleMs):
    // views that show it compare this value to see when time alone changed them.
    QByteArray freshness(qint64 nowMs) const;

    // Read replies, attention marks and pending reads at once, in one unread
    // pass; the same result as setting them one after another.
    void setReadState(const QJsonObject &read, const QJsonObject &marks, const QJsonObject &pending);
    void setReadReplies(const QJsonObject &read);
    QJsonObject readReplies() const { return m_readReplies; }
    // Replies Mark all as read may mark: unread, except in sessions marked as needing
    // attention. markRepliesRead checks both again when it applies a snapshot.
    QJsonObject unreadReplies() const;
    int markRepliesRead(const QJsonObject &replies);
    bool markReplyRead(const QString &host, const QString &name, const QString &conversation, const QString &reply);
    void setAttentionMarks(const QJsonObject &marks);
    QJsonObject attentionMarks() const { return m_attentionMarks; }
    bool markSessionRead(const QString &host, const SessionInfo &expected);
    bool setReviewLater(const QString &host, const SessionInfo &expected, bool marked);
    bool applyNotificationPreferences(const QString &host, const SessionInfo &expected, const QJsonObject &preferences);
    // Shared read state. Hosts with `hgs read` own unread/acknowledged state;
    // marks this GUI queued but no snapshot shows yet keep the UI read.
    void setPendingReads(const QJsonObject &pending);
    QJsonObject pendingReads() const { return m_pendingReads; }
    QList<ReadMark> takeNewReadMarks();       // queued by mark*Read since the last call
    QList<ReadMark> takeSettledReadMarks();   // shown by a snapshot or no longer current
    QList<ReadMark> legacyReadMarks(const QString &host) const;   // one-time QSettings migration
    bool supportsSharedRead(const QString &host) const;
    int totalSessions() const;
    int attentionSessions(qint64 nowMs) const;
    bool isStale(qint64 nowMs) const;            // хоть один объявленный пир протух
    QString tooltip(qint64 nowMs) const;         // "4 here, 2 on mac (20s ago)"

    const BoxState &local() const { return m_local; }
    QList<QString> peerNames() const;
    const BoxState *peer(const QString &alias) const;
    qint64 peerPolledAt(const QString &alias) const;

private:
    void updateUnread(const QString &host, BoxState &box);
    void queueRead(const QString &host, const SessionInfo &s, bool reply, bool attention);
    QJsonObject m_pendingReads;
    QList<ReadMark> m_newReads, m_settledReads;
    QJsonObject m_readReplies;
    QJsonObject m_attentionMarks;
    SessionInfo *matchingSession(const QString &host, const SessionInfo &expected);
    void updateAllUnread();
    // A box's revision changes whenever its listing content does (setLocal,
    // setPeer, applyNotificationPreferences), never on refreshListing. Values
    // are unique across all fleets, so equal revisions mean equal listings.
    struct PeerEntry { BoxState box; qint64 polledAt = 0; bool everPolled = false; quint64 revision = 0; };
    BoxState m_local;
    quint64 m_localRevision = 0;
    bool m_haveLocal = false;
    QMap<QString, PeerEntry> m_peers;
};
