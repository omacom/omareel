#pragma once
#include <QStringList>
class QGuiApplication;
int runCaptureTestClient(QGuiApplication &app, const QStringList &args, bool layer);
