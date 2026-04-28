use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;

#[test]
fn root_help_uses_requested_locale() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["--help", "--locale", "ar"]);
    cmd.assert()
        .success()
        .stdout(contains("واجهة سطر أوامر أدوات مطوري Greentic"))
        .stdout(contains("اطبع المساعدة"));
}

#[test]
fn secrets_help_uses_requested_locale() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["secrets", "--help", "--locale", "ar"]);
    cmd.assert()
        .success()
        .stdout(contains("مغلفات تسهيل الأسرار"))
        .stdout(contains("التفويض إلى greentic-secrets لتهيئة الأسرار لحزمة"))
        .stdout(contains("اطبع المساعدة"));
}

#[test]
fn config_help_uses_requested_locale() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["config", "--help", "--locale", "ar"]);
    cmd.assert()
        .success()
        .stdout(contains("إدارة إعدادات greentic-dev"))
        .stdout(contains("تعيين مفتاح في إعدادات greentic-dev"))
        .stdout(contains("اطبع المساعدة"));
}

#[test]
fn tools_help_uses_requested_locale() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["tools", "--help", "--locale", "ar"]);
    cmd.assert()
        .success()
        .stdout(contains(
            "تثبيت الملفات الثنائية لأدوات تطوير/تمهيد Greentic",
        ))
        .stdout(contains("تثبيت الأدوات من كتالوج أدوات Greentic المعتمد"))
        .stdout(contains("اطبع المساعدة"));
}

#[test]
fn coverage_help_uses_requested_locale() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["coverage", "--help", "--locale", "ar"]);
    cmd.assert()
        .success()
        .stdout(contains("تشغيل فحوصات التغطية مقابل coverage-policy.json"))
        .stdout(contains(
            "إعادة استخدام تقرير target/coverage/coverage.json موجود",
        ))
        .stdout(contains("اطبع المساعدة"));
}

#[test]
fn wizard_apply_help_uses_requested_locale() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["wizard", "apply", "--help", "--locale", "ar"]);
    cmd.assert()
        .success()
        .stdout(contains("تطبيق AnswerDocument للمشغل دون تفاعل"))
        .stdout(contains("ملف الإجابات"))
        .stdout(contains("تخطي مطالبة التأكيد التفاعلية"))
        .stdout(contains("اطبع المساعدة"));
}

#[test]
fn wizard_help_uses_requested_locale_for_answers_flag() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["wizard", "--help", "--locale", "ar"]);
    cmd.assert()
        .success()
        .stdout(contains("ملف الإجابات"))
        .stdout(contains("طباعة مخطط AnswerDocument الحالي"))
        .stdout(contains("وضع الواجهة الأمامية"))
        .stdout(contains("اطبع المساعدة"));
}

#[test]
fn secrets_runtime_error_uses_env_locale() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.env("LC_ALL", "ar")
        .env("GREENTIC_DEV_BIN_GREENTIC_SECRETS", "/tmp/does-not-exist")
        .args(["secrets", "init", "--pack", "dummy.gtpack"]);
    cmd.assert().failure().stderr(contains(
        "GREENTIC_DEV_BIN_GREENTIC_SECRETS يشير إلى ملف ثنائي غير موجود",
    ));
}
