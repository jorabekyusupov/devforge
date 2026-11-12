import json,pathlib,unittest
ROOT=pathlib.Path(__file__).resolve().parents[1]
class DistributionContracts(unittest.TestCase):
    def test_uninstaller_cannot_delete_application_data(self):
        template=(ROOT/'distribution/windows/installer.nsi').read_text()
        self.assertNotIn('DeleteAppDataCheckbox',template)
        self.assertNotIn('RmDir /r',template)
        self.assertNotIn('$(deleteAppData)',template)
        self.assertIn('NSIS_HOOK_PREUNINSTALL',template)
    def test_install_is_per_user_without_downgrades(self):
        config=json.loads((ROOT/'apps/desktop/src-tauri/tauri.windows.conf.json').read_text())['bundle']['windows']
        self.assertEqual(config['nsis']['installMode'],'currentUser')
        self.assertFalse(config['allowDowngrades'])
        self.assertEqual(config['nsis']['languages'],['English','Russian','Uzbek'])
        self.assertEqual(config['webviewInstallMode']['type'],'downloadBootstrapper')
    def test_uzbek_installer_covers_upstream_custom_keys(self):
        import re
        uz=(ROOT/'distribution/windows/Uzbek.nsh').read_text()
        keys=set(re.findall(r'^LangString (\w+)',uz,re.M))
        template=(ROOT/'distribution/windows/installer.nsi').read_text()
        referenced=set(re.findall(r'\$\((\w+)\)',template))
        self.assertTrue(referenced-{'PRODUCTNAME','VERSION'} <= keys | {'choowHowToInstall'})
    def test_all_packages_are_internal_and_no_automatic_updater(self):
        config=json.loads((ROOT/'apps/desktop/src-tauri/tauri.conf.json').read_text())
        self.assertNotIn('updater',config.get('plugins',{}))
        source=(ROOT/'scripts/package_desktop.py').read_text()
        self.assertIn("'public_release_allowed':False",source)
        self.assertIn("'devforge-helper'",source)
if __name__=='__main__':unittest.main()
