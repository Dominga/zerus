#include <QtTest>
#include "ConversationSource.h"
#include "IdentityBadge.h"
class TestConversationSource:public QObject {
    Q_OBJECT
private slots:
    void explicitKindsAreDistinct(){
        QSet<QString> labels;
        for(const auto &kind:{"chatgpt_chat","chatgpt_work","codex_chat"}){
            QJsonObject context{{"schema_version",1},{"kind",kind},{"evidence","explicit_registration"}};
            const auto label=ConversationSource::label("external",context);QVERIFY(!label.isEmpty());labels.insert(label);
            QCOMPARE(ConversationSource::badge("external",context),QString(kind));
        }
        QCOMPARE(labels.size(),3);
    }
    void nativeCodexNeverBecomesChatOrWorkByModelName(){
        const QJsonObject spoof{{"schema_version",1},{"kind","chatgpt_work"},{"evidence","explicit_registration"},{"model","codex"}};
        QCOMPARE(ConversationSource::badge("codex",spoof),QString("codex"));
        QVERIFY(ConversationSource::label("codex",spoof).isEmpty());
        QCOMPARE(IdentityBadges::providerName("codex"),QString("Codex"));
    }
    void unknownAndFutureMetadataStaysUnknown(){
        for(const auto &context:{QJsonObject{},QJsonObject{{"schema_version",2},{"kind","chatgpt_chat"},{"evidence","explicit_registration"}},QJsonObject{{"schema_version",true},{"kind","chatgpt_chat"},{"evidence","explicit_registration"}},QJsonObject{{"schema_version",1},{"kind","future"},{"evidence","explicit_registration"}}}){
            QCOMPARE(ConversationSource::badge("external",context),QString("external"));
        }
    }
    void cooperativeAdmissionNeedsExactDeclaredProtocolNotALiveProcess(){
        QJsonObject raw{{"agent","external"},{"backend","external-workstream"},{"external_protocol","cooperative-inbox-v1"},
            {"send_delivery","cooperative_inbox"},{"external_cloud_control",false},{"tracked",true},{"send_available",true},{"send_supported",true},
            {"run_id","exact-run"},{"conversation_id","exact-conversation"},{"client_context",QJsonObject{{"schema_version",1},{"kind","chatgpt_chat"},{"evidence","explicit_registration"}}}};
        QVERIFY(ConversationSource::cooperative(raw));
        raw["external_protocol"]="future";QVERIFY(!ConversationSource::cooperative(raw));
        raw["external_protocol"]="cooperative-inbox-v1";raw["external_cloud_control"]=true;QVERIFY(!ConversationSource::cooperative(raw));
    }
    void chatLinksCannotBecomeCommandsOrOtherOrigins(){
        QVERIFY(ConversationSource::chatUrl("https://chatgpt.com/c/11111111-2222-4333-8444-555555555555").isValid());
        for(const auto &value:{"file:///tmp/file","https://chatgpt.com.evil.test/c/11111111-2222-4333-8444-555555555555","https://chatgpt.com/c/11111111-2222-4333-8444-555555555555?token=x"})QVERIFY(ConversationSource::chatUrl(value).isEmpty());
    }
};
QTEST_MAIN(TestConversationSource)
#include "test_conversationsource.moc"
