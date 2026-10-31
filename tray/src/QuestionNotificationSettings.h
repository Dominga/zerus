#pragma once
#include "Settings.h"
#include <QSettings>

// Banners for new optional (async, skippable) questions are off by default;
// Settings → Sessions turns them on for this Zerus.
namespace QuestionNotificationSettings {
inline bool enabled(){return Settings::value("notifications/optionalQuestions",false).toBool();}
inline void setEnabled(bool on){Settings::set("notifications/optionalQuestions",on);}
}
