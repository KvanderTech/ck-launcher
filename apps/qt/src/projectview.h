#pragma once
#include "ui.h"

class ProjectView final : public QWidget {
    Q_OBJECT
  public:
    ProjectView(Backend *backend, ImagePool *images, QWidget *parent = nullptr);
    void open(const QJsonObject &project, const QJsonArray &builds, const QString &selected);
    void setInstalling(bool active);
    static bool compatible(const QJsonObject &version, const QJsonObject &build,
                           const QString &type);
  signals:
    void back();
    void installRequested(const QString &project, const QString &version, const QString &build,
                          bool modpack);

  private:
    Backend *backend;
    ImagePool *images;
    QJsonObject project;
    QJsonArray allVersions, builds;
    quint64 generation = 0;
    Picture *icon;
    QLabel *title, *description, *status;
    QTextBrowser *body;
    QComboBox *game, *loader, *version, *target;
    QPushButton *install;
    QTableWidget *versionTable;
    QTabBar *tabs;
    void filterVersions();
    void targetChanged();
    void installSelected();
};
